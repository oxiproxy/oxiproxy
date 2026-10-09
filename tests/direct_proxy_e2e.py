#!/usr/bin/env python3
"""Exercise Controller + Node without creating any Client.

Run after building the dashboard and `cargo build -p controller -p node`.
Use --public-origin https://example.com for an additional live HTTPS origin check.
All state and logs are temporary; credentials are never printed.
"""
import argparse
import http.client as http_client
import json
import os
from pathlib import Path
import re
import secrets
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[1]


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def wait_until(check, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        try:
            result = check()
            if result:
                return result
        except (OSError, http_client.HTTPException):
            pass
        time.sleep(0.2)
    raise AssertionError("Timed out waiting for validation condition")


def http(port, method, path, payload=None, headers=None):
    conn = http_client.HTTPConnection("127.0.0.1", port, timeout=15)
    try:
        body = json.dumps(payload).encode() if payload is not None else None
        conn.request(method, path, body, {"Content-Type": "application/json", **(headers or {})})
        response = conn.getresponse()
        return response.status, response.read()
    finally:
        conn.close()


def stop(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def origin(label):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def do_GET(self):
            size = int(self.headers.get("Content-Length", 0))
            body = json.dumps({"origin": label, "host": self.headers["Host"],
                               "path": self.path, "body": self.rfile.read(size).decode()}).encode()
            self.send_response(200)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        do_POST = do_GET

        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--public-origin")
    args = parser.parse_args()
    processes = []
    servers = [origin("A"), origin("B")]
    env = {**os.environ, "JWT_SECRET": secrets.token_hex(32), "RUST_LOG": "warn"}
    env.pop("LD_LIBRARY_PATH", None)
    try:
        with tempfile.TemporaryDirectory(prefix="direct-proxy-e2e-", dir=ROOT / "target") as directory:
            work = Path(directory)
            web, grpc, tunnel, entry = (free_port() for _ in range(4))
            (work / "controller.toml").write_text(f"web_port = {web}\ninternal_port = {grpc}\n")

            def spawn(binary, *argv):
                log = open(work / f"{binary}-{len(processes)}.log", "wb")
                os.chmod(log.name, 0o600)
                process = subprocess.Popen([str(ROOT / "target/debug" / binary), *argv], cwd=work,
                                           env=env, stdout=log, stderr=subprocess.STDOUT)
                log.close()
                processes.append(process)
                return process

            controller = spawn("controller", "start")
            wait_until(lambda: (work / "data/admin_password.txt").exists())
            password = re.search(r"密码: (\S+)", (work / "data/admin_password.txt").read_text())[1]
            wait_until(lambda: http(web, "GET", "/")[0] == 200)

            def api(method, path, data=None, token=None, expected=200):
                status, body = http(web, method, "/api" + path, data,
                                    {"Authorization": f"Bearer {token}"} if token else {})
                assert status == expected, f"{method} {path}: expected {expected}, got {status}"
                value = json.loads(body)
                if expected == 200:
                    assert value["success"], f"{method} {path}: unsuccessful API response"
                return value.get("data")

            admin = api("POST", "/auth/login", {"username": "admin", "password": password})["token"]
            users = []
            for username in ["owner", "other"]:
                password = secrets.token_urlsafe(20)
                user = api("POST", "/users", {"username": username, "password": password,
                           "max_port_count": 3, "max_client_count": 0, "traffic_quota_gb": 1}, admin)
                token = api("POST", "/auth/login", {"username": username, "password": password})["token"]
                users.append((user["id"], token))
            owner_id, owner = users[0]
            _, other = users[1]
            node = api("POST", "/nodes", {"name": "validation", "url": "http://127.0.0.1",
                       "tunnelAddr": "127.0.0.1", "tunnelPort": tunnel,
                       "tunnelProtocol": "tcp", "nodeType": "shared", "maxProxyCount": 3}, admin)

            def start_node():
                process = spawn("node", "start", "--controller-url", f"http://127.0.0.1:{grpc}",
                                "--token", node["secret"], "--protocol", "tcp")
                wait_until(lambda: any(n["id"] == node["id"] and n["isOnline"]
                                       for n in api("GET", "/nodes", token=admin)))
                return process

            worker = start_node()
            assert api("GET", "/clients", token=admin) == []
            origin_a, origin_b = (f"http://127.0.0.1:{s.server_port}" for s in servers)
            config = {"name": "without-client", "type": "http", "domain": "app.test",
                      "nodeId": node["id"], "remotePort": entry, "upstreamUrl": origin_a}
            proxy = api("POST", "/proxies", config, owner)
            assert proxy["client_id"] is None and proxy["userId"] == owner_id
            assert len(api("GET", "/proxies", token=owner)) == 1
            assert api("GET", "/proxies", token=other) == []
            assert api("GET", f"/dashboard/stats/{owner_id}", token=owner)["total_proxies"] == 1
            api("PUT", f"/proxies/{proxy['id']}", {"enabled": False}, other, expected=403)
            api("DELETE", f"/proxies/{proxy['id']}", token=other, expected=403)
            api("POST", "/proxies", config, owner, expected=409)
            for invalid in ["example.com", "ftp://example.com", "https://user:pass@example.com", "http://example.com/api"]:
                api("POST", "/proxies", {**config, "domain": "invalid.test", "upstreamUrl": invalid}, owner, expected=400)
            api("POST", "/proxies", {**config, "client_id": "1"}, owner, expected=400)
            api("POST", "/proxies", {**config, "type": "https", "upstreamUrl": origin_a}, owner, expected=400)

            def forwarded(label):
                status, body = http(entry, "POST", "/api?q=test", {"test": True}, {"Host": "app.test"})
                assert status == 200, f"upstream request returned {status}"
                result = json.loads(body)
                assert result["origin"] == label and result["path"] == "/api?q=test"
                assert json.loads(result["body"]) == {"test": True}
                return True

            forwarded("A")
            api("PUT", f"/proxies/{proxy['id']}", {"upstreamUrl": origin_b}, owner)
            forwarded("B")
            # A bind failure must restore the original configuration and listener.
            api("PUT", f"/proxies/{proxy['id']}", {"remotePort": servers[0].server_port}, owner, expected=409)
            assert api("GET", "/proxies", token=owner)[0]["remotePort"] == entry
            forwarded("B")

            extra = api("POST", "/proxies", {**config, "domain": "other.test"}, owner)
            api("PUT", f"/proxies/{proxy['id']}", {"enabled": False}, owner)
            assert http(entry, "GET", "/", headers={"Host": "app.test"})[0] == 404
            assert http(entry, "GET", "/", headers={"Host": "other.test"})[0] == 200
            api("PUT", f"/proxies/{proxy['id']}", {"enabled": True}, owner)
            forwarded("B")
            third = api("POST", "/proxies", {**config, "domain": "third.test",
                        "upstreamUrl": args.public_origin or origin_a}, owner)
            api("POST", "/proxies", {**config, "domain": "over-quota.test"}, owner, expected=403)
            # Updating an existing rule must work at the node's rule limit.
            alternate_entry = free_port()
            api("PUT", f"/proxies/{proxy['id']}", {"remotePort": alternate_entry}, owner)
            api("PUT", f"/proxies/{proxy['id']}", {"remotePort": entry}, owner)
            api("PUT", f"/users/{owner_id}", {"allowed_port_range": str(entry)}, admin)
            api("PUT", f"/proxies/{proxy['id']}", {"remotePort": alternate_entry}, owner, expected=403)
            api("PUT", f"/users/{owner_id}", {"allowed_port_range": ""}, admin)
            api("PUT", f"/proxies/{proxy['id']}", {"enabled": False}, owner)
            fourth = api("POST", "/proxies", {**config, "domain": "fourth.test"}, admin)
            api("PUT", f"/proxies/{proxy['id']}", {"enabled": True}, owner, expected=403)
            api("DELETE", f"/proxies/{fourth['id']}", token=admin)
            api("PUT", f"/proxies/{proxy['id']}", {"enabled": True}, owner)
            info = api("GET", "/users", token=admin)
            assert next(u for u in info if u["id"] == owner_id)["currentPortCount"] == 3
            if args.public_origin:
                assert http(entry, "GET", "/", headers={"Host": "third.test"})[0] == 200
                print("PASS: real public HTTPS upstream")

            def traffic_recorded():
                info = api("GET", "/traffic/overview", token=owner)
                records = info["by_proxy"]
                return any(p["proxy_id"] == proxy["id"] and p["total_bytes"] > 0 for p in records) and bool(info["daily_traffic"])

            wait_until(traffic_recorded, timeout=15)
            assert api("GET", "/traffic/overview", token=other)["by_proxy"] == []
            api("PUT", f"/users/{owner_id}", {"traffic_quota_gb": 0.000000001}, admin)
            assert http(entry, "GET", "/", headers={"Host": "app.test"})[0] == 403
            api("PUT", f"/users/{owner_id}", {"traffic_quota_gb": 1}, admin)
            forwarded("B")

            stop(worker)
            wait_until(lambda: not next(n for n in api("GET", "/nodes", token=admin) if n["id"] == node["id"])["isOnline"])
            worker = start_node()
            wait_until(lambda: forwarded("B"))

            # A Controller restart exercises reconciliation on the same Node.
            with sqlite3.connect(work / "data/oxiproxy.db") as db:
                for key, value in [("web_port", web), ("internal_port", grpc)]:
                    db.execute("UPDATE system_config SET value=? WHERE key=?", (str(value), key))
            stop(controller)
            controller = spawn("controller", "start")
            wait_until(lambda: http(web, "GET", "/")[0] == 200)
            wait_until(lambda: any(n["id"] == node["id"] and n["isOnline"] for n in api("GET", "/nodes", token=admin)))
            wait_until(lambda: http(entry, "GET", "/", headers={"Host": "app.test"})[0] == 200)
            forwarded("B")

            for rule in [extra, third, proxy]:
                api("DELETE", f"/proxies/{rule['id']}", token=owner)
            with socket.socket() as probe:
                probe.bind(("0.0.0.0", entry))
            assert api("GET", "/clients", token=admin) == []
            api("POST", "/proxies", config, owner)
            api("DELETE", f"/users/{owner_id}", token=admin)
            with socket.socket() as probe:
                probe.bind(("0.0.0.0", entry))
            assert api("GET", "/proxies", token=admin) == []
            with sqlite3.connect(work / "data/oxiproxy.db") as db:
                assert db.execute("PRAGMA integrity_check").fetchone()[0] == "ok"
                assert db.execute("PRAGMA foreign_key_check").fetchall() == []
            print("PASS: clientless create/edit/rollback/disable/enable/delete, owner isolation,")
            print("      port and traffic quotas, statistics, Node/Controller recovery, database integrity")
            for process in processes:
                stop(process)
    finally:
        for process in processes:
            stop(process)
        for server in servers:
            server.shutdown()
            server.server_close()


if __name__ == "__main__":
    main()
