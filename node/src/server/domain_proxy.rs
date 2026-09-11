//! Shared HTTP/1 Host routing and TLS SNI passthrough. No website keys are held here.
use super::{
    proxy_server::{ConnectionProvider, handle_tcp_to_tunnel_unified},
    speed_limiter::SpeedLimiter,
    traffic::TrafficManager,
};
use anyhow::{Result, anyhow, bail, ensure};
use bytes::Bytes;
use common::{
    domain::{find_route, normalize_domain, normalize_pattern},
    protocol::control::ProxyConfig,
};
use http_body_util::{BodyExt, Full, combinators::UnsyncBoxBody};
use hyper::{Request, Response, StatusCode, body::Incoming, header};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{
    collections::HashMap, convert::Infallible, io::Cursor, net::SocketAddr, sync::Arc,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{TcpListener, TcpStream},
    sync::{Mutex, RwLock, Semaphore},
    task::JoinHandle,
};

const HELLO_LIMIT: usize = 65536;
const HEADER_TIMEOUT: Duration = Duration::from_secs(10);
type Routes = Arc<RwLock<HashMap<String, Route>>>;
type Body = UnsyncBoxBody<Bytes, hyper::Error>;

#[derive(Clone)]
struct Route {
    config: ProxyConfig,
    provider: ConnectionProvider,
    traffic: Arc<TrafficManager>,
    limiter: Arc<SpeedLimiter>,
}

struct Listener {
    kind: String,
    routes: Routes,
    task: JoinHandle<()>,
}
impl Drop for Listener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Default)]
pub struct DomainListeners(Mutex<HashMap<u16, Listener>>);

impl DomainListeners {
    pub async fn start(
        &self,
        mut config: ProxyConfig,
        provider: ConnectionProvider,
        traffic: Arc<TrafficManager>,
        limiter: Arc<SpeedLimiter>,
    ) -> Result<()> {
        config.domain = normalize_pattern(&config.domain).map_err(|e| anyhow!(e))?;
        let mut listeners = self.0.lock().await;
        if let Some(listener) = listeners.get(&config.remote_port) {
            ensure!(listener.kind == config.proxy_type, "共享端口的协议必须一致");
            let mut routes = listener.routes.write().await;
            if let Some(route) = routes.get(&config.domain) {
                ensure!(
                    route.config.proxy_id == config.proxy_id
                        && route.config.client_id == config.client_id,
                    "域名已被其他代理使用"
                );
            }
            routes.insert(
                config.domain.clone(),
                Route {
                    config,
                    provider,
                    traffic,
                    limiter,
                },
            );
            return Ok(());
        }
        // Keep the bound socket: no preflight/drop/rebind race.
        let socket = TcpListener::bind(("0.0.0.0", config.remote_port)).await?;
        let port = config.remote_port;
        let kind = config.proxy_type.clone();
        let routes: Routes = Arc::default();
        routes.write().await.insert(
            config.domain.clone(),
            Route {
                config,
                provider,
                traffic,
                limiter,
            },
        );
        let accept_routes = routes.clone();
        let is_tls = kind == "https";
        let task = tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(1024));
            loop {
                let (stream, addr) = match socket.accept().await {
                    Ok(connection) => connection,
                    Err(error) => {
                        tracing::warn!(%port, %error, "共享端口 accept 失败");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                };
                let Ok(permit) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let routes = accept_routes.clone();
                tokio::spawn(async move {
                    let _permit = permit;
                    let result = if is_tls {
                        serve_tls(stream, addr, routes).await
                    } else {
                        serve_http(stream, addr, routes).await
                    };
                    if let Err(error) = result {
                        tracing::debug!(%addr, %error, "域名代理连接结束");
                    }
                });
            }
        });
        listeners.insert(port, Listener { kind, routes, task });
        Ok(())
    }

    pub async fn stop(&self, client: &str, proxy: Option<i64>) {
        let mut listeners = self.0.lock().await;
        let mut empty = Vec::new();
        for (port, listener) in listeners.iter() {
            let mut routes = listener.routes.write().await;
            routes.retain(|_, route| {
                !(route.config.client_id == client
                    && proxy.is_none_or(|id| id == route.config.proxy_id))
            });
            if routes.is_empty() {
                empty.push(*port);
            }
        }
        for port in empty {
            if let Some(mut listener) = listeners.remove(&port) {
                listener.task.abort();
                let _ = (&mut listener.task).await; // release socket before allowing a restart
            }
        }
    }
}

async fn forward<S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send>(
    stream: S,
    addr: SocketAddr,
    route: Route,
) -> Result<()> {
    let config = route.config;
    handle_tcp_to_tunnel_unified(
        stream,
        addr,
        format!("{}:{}", config.local_ip, config.local_port),
        config.name,
        config.client_id,
        route.provider,
        config.proxy_id,
        route.traffic,
        route.limiter,
    )
    .await
}

// rustls parses fragmented TLS records and ClientHello extensions without terminating TLS.
async fn read_sni(stream: &mut TcpStream) -> Result<(String, Vec<u8>)> {
    let mut acceptor = rustls::server::Acceptor::default();
    let mut prefix = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = stream.read(&mut buf).await?;
        ensure!(n > 0, "TLS 握手不完整");
        ensure!(prefix.len() + n <= HELLO_LIMIT, "TLS ClientHello 过大");
        prefix.extend_from_slice(&buf[..n]);
        acceptor.read_tls(&mut Cursor::new(&buf[..n]))?;
        match acceptor.accept() {
            Ok(Some(accepted)) => {
                let name = accepted
                    .client_hello()
                    .server_name()
                    .ok_or_else(|| anyhow!("TLS 缺少 SNI"))?
                    .to_owned();
                return Ok((normalize_domain(&name).map_err(|e| anyhow!(e))?, prefix));
            }
            Ok(None) => {}
            Err(_) => bail!("无效 TLS ClientHello"),
        }
    }
}

async fn serve_tls(mut stream: TcpStream, addr: SocketAddr, routes: Routes) -> Result<()> {
    let (domain, prefix) = tokio::time::timeout(HEADER_TIMEOUT, read_sni(&mut stream)).await??;
    let route = find_route(&*routes.read().await, &domain)
        .cloned()
        .ok_or_else(|| anyhow!("SNI 未匹配路由"))?;
    // Replay every sniffed byte, including bytes following ClientHello in the same read.
    let (read, write) = stream.into_split();
    forward(
        tokio::io::join(Cursor::new(prefix).chain(read), write),
        addr,
        route,
    )
    .await
}

fn response(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(
        Full::new(Bytes::from(format!("{}\n", status)))
            .map_err(|never| match never {})
            .boxed_unsync(),
    );
    *response.status_mut() = status;
    response
}

fn request_domain<B>(request: &Request<B>) -> Result<String> {
    let mut hosts = request.headers().get_all(header::HOST).iter();
    let host = hosts.next().ok_or_else(|| anyhow!("缺少 Host"))?.to_str()?;
    ensure!(hosts.next().is_none(), "重复 Host");
    let authority: hyper::http::uri::Authority = host.parse()?;
    ensure!(!host.contains('@'), "无效 Host");
    let domain = normalize_domain(authority.host()).map_err(|e| anyhow!(e))?;
    if let Some(uri_authority) = request.uri().authority() {
        ensure!(
            uri_authority.as_str().eq_ignore_ascii_case(host),
            "请求 URI 与 Host 不一致"
        );
    }
    Ok(domain)
}

fn strip_hop_headers(headers: &mut hyper::HeaderMap, upgrade: bool) {
    let names: Vec<String> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|name| name.trim().to_ascii_lowercase())
        .collect();
    for name in names {
        if !upgrade || name != "upgrade" {
            headers.remove(name);
        }
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        if !upgrade || !matches!(name, "connection" | "upgrade") {
            headers.remove(name);
        }
    }
    if upgrade {
        headers.insert(
            header::CONNECTION,
            header::HeaderValue::from_static("upgrade"),
        );
    }
}

async fn http_request(
    mut request: Request<Incoming>,
    addr: SocketAddr,
    routes: Routes,
) -> Result<Response<Body>> {
    let domain = match request_domain(&request) {
        Ok(domain) => domain,
        Err(_) => return Ok(response(StatusCode::BAD_REQUEST)),
    };
    if request.method() == hyper::Method::CONNECT {
        return Ok(response(StatusCode::METHOD_NOT_ALLOWED));
    }
    let route = match find_route(&*routes.read().await, &domain).cloned() {
        Some(route) => route,
        None => return Ok(response(StatusCode::NOT_FOUND)),
    };
    if !route.provider.is_online(&route.config.client_id).await {
        return Ok(response(StatusCode::SERVICE_UNAVAILABLE));
    }
    let upgrade = request
        .headers()
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|value| {
            value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        })
        && request.headers().contains_key(header::UPGRADE);
    if upgrade
        && !request.headers()[header::UPGRADE]
            .to_str()
            .unwrap_or("")
            .eq_ignore_ascii_case("websocket")
    {
        return Ok(response(StatusCode::BAD_REQUEST));
    }
    let downstream_upgrade = upgrade.then(|| hyper::upgrade::on(&mut request));
    let host = request.headers()[header::HOST].clone();
    strip_hop_headers(request.headers_mut(), upgrade);
    request.headers_mut().insert(header::HOST, host);
    if !upgrade {
        request.headers_mut().insert(
            header::CONNECTION,
            header::HeaderValue::from_static("close"),
        );
    }
    let path = request
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/")
        .parse()?;
    *request.uri_mut() = path;
    // One upstream connection per request, so keep-alive requests can select different clients.
    let (upstream, bridge) = tokio::io::duplex(32768);
    let forwarding = tokio::spawn(async move {
        let _ = forward(bridge, addr, route).await;
    });
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(upstream)).await?;
    tokio::spawn(async move {
        let _ = connection.with_upgrades().await;
        // Do not abort forwarding: upgrades and half-close traffic still need the stream.
        let _ = forwarding.await;
    });
    let mut result = sender.send_request(request).await?;
    if result.status() == StatusCode::SWITCHING_PROTOCOLS {
        let downstream = downstream_upgrade.ok_or_else(|| anyhow!("意外的协议升级"))?;
        let upstream = hyper::upgrade::on(&mut result);
        tokio::spawn(async move {
            if let (Ok(downstream), Ok(upstream)) = tokio::join!(downstream, upstream) {
                let _ = tokio::io::copy_bidirectional(
                    &mut TokioIo::new(downstream),
                    &mut TokioIo::new(upstream),
                )
                .await;
            }
        });
        strip_hop_headers(result.headers_mut(), true);
    } else {
        strip_hop_headers(result.headers_mut(), false);
    }
    Ok(result.map(|body| body.boxed_unsync()))
}

async fn serve_http(stream: TcpStream, addr: SocketAddr, routes: Routes) -> Result<()> {
    let service = hyper::service::service_fn(move |request| {
        let routes = routes.clone();
        async move {
            Ok::<_, Infallible>(match http_request(request, addr, routes).await {
                Ok(response) => response,
                Err(error) => {
                    tracing::debug!(%error, "HTTP 上游请求失败");
                    response(StatusCode::BAD_GATEWAY)
                }
            })
        }
    });
    hyper::server::conn::http1::Builder::new()
        .timer(TokioTimer::new())
        .header_read_timeout(HEADER_TIMEOUT)
        .max_buf_size(32768)
        .serve_connection(TokioIo::new(stream), service)
        .with_upgrades()
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
