use super::*;
use async_trait::async_trait;
use common::{TunnelConnection, TunnelRecvStream, TunnelSendStream, TunnelStream};
use tokio::io::{AsyncWriteExt, DuplexStream, ReadHalf, WriteHalf};

// A real byte-stream tunnel peer, including the production proxy preface.
struct Peer;
struct Send(WriteHalf<DuplexStream>);
struct Recv(ReadHalf<DuplexStream>);
#[async_trait]
impl TunnelSendStream for Send {
    async fn write_all(&mut self, data: &[u8]) -> Result<()> {
        self.0.write_all(data).await?;
        Ok(())
    }
    async fn flush(&mut self) -> Result<()> {
        self.0.flush().await?;
        Ok(())
    }
    async fn finish(&mut self) -> Result<()> {
        self.0.shutdown().await?;
        Ok(())
    }
}
#[async_trait]
impl TunnelRecvStream for Recv {
    async fn read_exact(&mut self, data: &mut [u8]) -> Result<()> {
        self.0.read_exact(data).await?;
        Ok(())
    }
    async fn read(&mut self, data: &mut [u8]) -> Result<Option<usize>> {
        let n = self.0.read(data).await?;
        Ok((n != 0).then_some(n))
    }
}
#[async_trait]
impl TunnelConnection for Peer {
    async fn open_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        let (stream, mut peer) = tokio::io::duplex(32768);
        tokio::spawn(async move {
            let mut preface = [0; 4];
            peer.read_exact(&mut preface).await.unwrap();
            assert_eq!(&preface[..2], b"pt");
            let mut target = vec![0; u16::from_be_bytes([preface[2], preface[3]]) as usize];
            peer.read_exact(&mut target).await.unwrap();
            let mut backend = TcpStream::connect(String::from_utf8(target).unwrap())
                .await
                .unwrap();
            let _ = tokio::io::copy_bidirectional(&mut peer, &mut backend).await;
        });
        let (read, write) = tokio::io::split(stream);
        Ok((Box::new(Send(write)), Box::new(Recv(read))))
    }
    async fn accept_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        bail!("unused")
    }
    async fn open_bi_stream(&self) -> Result<Box<dyn TunnelStream>> {
        bail!("unused")
    }
    async fn accept_bi_stream(&self) -> Result<Box<dyn TunnelStream>> {
        bail!("unused")
    }
    async fn open_uni(&self) -> Result<Box<dyn TunnelSendStream>> {
        bail!("unused")
    }
    async fn accept_uni(&self) -> Result<Box<dyn TunnelRecvStream>> {
        bail!("unused")
    }
    fn remote_address(&self) -> SocketAddr {
        "127.0.0.1:1".parse().unwrap()
    }
    fn close_reason(&self) -> Option<String> {
        None
    }
}

fn fixture() -> (ConnectionProvider, Arc<TrafficManager>, Arc<SpeedLimiter>) {
    let peers = Arc::new(RwLock::new(HashMap::from([
        (
            "1".into(),
            Arc::new(Box::new(Peer) as Box<dyn TunnelConnection>),
        ),
        (
            "2".into(),
            Arc::new(Box::new(Peer) as Box<dyn TunnelConnection>),
        ),
    ])));
    let (tx, _rx) = tokio::sync::mpsc::channel(100);
    (
        ConnectionProvider::new(Arc::default(), peers),
        Arc::new(TrafficManager::new(
            super::super::grpc_client::SharedGrpcSender::new(tx),
        )),
        SpeedLimiter::new(0),
    )
}
fn config(client: i64, port: u16, backend: u16, kind: &str, domain: &str) -> ProxyConfig {
    ProxyConfig {
        proxy_id: client,
        client_id: client.to_string(),
        name: domain.into(),
        proxy_type: kind.into(),
        domain: domain.into(),
        local_ip: "127.0.0.1".into(),
        local_port: backend,
        remote_port: port,
        enabled: true,
    }
}
async fn port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
async fn backend(label: &'static str) -> (u16, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let task = tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let service =
                    hyper::service::service_fn(move |mut req: Request<Incoming>| async move {
                        let mut response = Response::new(Full::new(Bytes::from(label)));
                        if req.uri().path() == "/ws" {
                            let upgrade = hyper::upgrade::on(&mut req);
                            tokio::spawn(async move {
                                let mut stream = TokioIo::new(upgrade.await.unwrap());
                                let mut data = [0; 4];
                                stream.read_exact(&mut data).await.unwrap();
                                stream.write_all(&data).await.unwrap();
                            });
                            *response.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
                            response.headers_mut().insert(
                                header::CONNECTION,
                                header::HeaderValue::from_static("upgrade"),
                            );
                            response.headers_mut().insert(
                                header::UPGRADE,
                                header::HeaderValue::from_static("websocket"),
                            );
                        } else {
                            let host = req
                                .headers()
                                .get(header::HOST)
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .to_owned();
                            let body = req.into_body().collect().await.unwrap().to_bytes();
                            *response.body_mut() = Full::new(Bytes::from(format!(
                                "{label}:{host}:{}",
                                String::from_utf8_lossy(&body)
                            )));
                        }
                        Ok::<_, Infallible>(response)
                    });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), service)
                    .with_upgrades()
                    .await;
            });
        }
    });
    (port, task)
}

#[tokio::test]
async fn http_routes_each_keepalive_request_and_preserves_bodies_and_upgrades() {
    check_http_routes(false).await;
}

#[tokio::test]
async fn http_wildcard_and_exact_routes_share_listener() {
    check_http_routes(true).await;
}

async fn check_http_routes(wildcard: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let manager = DomainListeners::default();
        let (provider, traffic, limiter) = fixture();
        let port = port().await;
        let (a, task_a) = backend("A").await;
        let (b, task_b) = backend("B").await;
        for (id, backend, domain) in [(2, b, "b.test"), (1, a, if wildcard { "*.test" } else { "a.test" })] {
            manager
                .start(
                    config(id, port, backend, "http", domain),
                    provider.clone(),
                    traffic.clone(),
                    limiter.clone(),
                )
                .await
                .unwrap();
        }
        assert!(manager
            .start(
                config(3, port, b, "http", if wildcard { "*.TEST." } else { "A.TEST." }),
                provider.clone(),
                traffic.clone(),
                limiter.clone()
            )
            .await
            .is_err());
        assert!(manager
            .start(
                config(3, port, b, "https", "c.test"),
                provider.clone(),
                traffic.clone(),
                limiter.clone()
            )
            .await
            .is_err());
        let stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = conn.with_upgrades().await;
        });
        for (host, expected) in [
            ("A.TEST.:80", "A:A.TEST.:80:hello"),
            ("b.test", "B:b.test:hello"),
            (if wildcard { "deep.a.test" } else { "a.test" }, if wildcard { "A:deep.a.test:hello" } else { "A:a.test:hello" }),
        ] {
            let req = Request::post("/body")
                .header("Host", host)
                .body(Full::new(Bytes::from_static(b"hello")))
                .unwrap();
            let result = sender.send_request(req).await.unwrap();
            assert_eq!(result.status(), StatusCode::OK);
            assert_eq!(
                result.into_body().collect().await.unwrap().to_bytes(),
                expected
            );
        }
        let unknown = Request::get("/")
            .header("Host", "unknown.example")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            sender.send_request(unknown).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        // Chunked upload followed by a pipelined request for another client.
        let mut pipeline = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        pipeline.write_all(b"POST /body HTTP/1.1\r\nHost: a.test\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhe\r\n3\r\nllo\r\n0\r\n\r\nGET / HTTP/1.1\r\nHost: b.test\r\nConnection: close\r\n\r\n").await.unwrap();
        let mut responses = String::new();
        pipeline.read_to_string(&mut responses).await.unwrap();
        assert_eq!(responses.matches("HTTP/1.1 200").count(), 2);
        assert!(responses.contains("A:a.test:hello"));
        assert!(responses.contains("B:b.test:"));
        manager.stop("1", None).await;
        let req = Request::get("/")
            .header("Host", "b.test")
            .body(Full::new(Bytes::new()))
            .unwrap();
        assert_eq!(
            sender
                .send_request(req)
                .await
                .unwrap()
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes(),
            "B:b.test:"
        );
        let req = Request::get("/ws")
            .header("Host", "b.test")
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .body(Full::new(Bytes::new()))
            .unwrap();
        let mut result = sender.send_request(req).await.unwrap();
        assert_eq!(result.status(), StatusCode::SWITCHING_PROTOCOLS);
        let mut upgraded = TokioIo::new(hyper::upgrade::on(&mut result).await.unwrap());
        upgraded.write_all(b"ping").await.unwrap();
        let mut echo = [0; 4];
        upgraded.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"ping");
        manager.stop("2", Some(2)).await;
        assert!(manager.0.lock().await.is_empty());
        let _rebind = TcpListener::bind(("0.0.0.0", port)).await.unwrap();
        task_a.abort();
        task_b.abort();
    })
    .await
    .unwrap();
}

fn client_hello(domain: &str, sni: bool) -> Vec<u8> {
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_root_certificates(rustls::RootCertStore::empty())
    .with_no_client_auth();
    config.enable_sni = sni;
    let mut client =
        rustls::ClientConnection::new(Arc::new(config), domain.to_owned().try_into().unwrap())
            .unwrap();
    let mut hello = Vec::new();
    client.write_tls(&mut hello).unwrap();
    hello
}

#[tokio::test]
async fn tls_fragmented_clienthello_routes_and_replays_exact_bytes() {
    check_tls_routes(false).await;
}

#[tokio::test]
async fn tls_wildcard_and_exact_routes_replay_clienthello() {
    check_tls_routes(true).await;
}

async fn check_tls_routes(wildcard: bool) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let manager = DomainListeners::default();
        let (provider, traffic, limiter) = fixture();
        let port = port().await;
        for (id, domain) in [(1, "a.test"), (2, "b.test")] {
            let backend = TcpListener::bind("127.0.0.1:0").await.unwrap();
            manager
                .start(
                    config(
                        id,
                        port,
                        backend.local_addr().unwrap().port(),
                        "https",
                        if wildcard && id == 1 {
                            "*.test"
                        } else {
                            domain
                        },
                    ),
                    provider.clone(),
                    traffic.clone(),
                    limiter.clone(),
                )
                .await
                .unwrap();
            let hello = client_hello(domain, true);
            // Split the handshake across TLS records as well as across TCP writes.
            let record_len = u16::from_be_bytes([hello[3], hello[4]]) as usize;
            let mut wire = Vec::new();
            for part in hello[5..5 + record_len].chunks(23) {
                wire.extend_from_slice(&hello[..3]);
                wire.extend_from_slice(&(part.len() as u16).to_be_bytes());
                wire.extend_from_slice(part);
            }
            wire.extend_from_slice(&hello[5 + record_len..]);
            let expected = wire.clone();
            let peer = tokio::spawn(async move {
                let (mut stream, _) = backend.accept().await.unwrap();
                let mut received = vec![0; expected.len()];
                stream.read_exact(&mut received).await.unwrap();
                assert_eq!(received, expected);
                stream.write_all(domain.as_bytes()).await.unwrap();
                stream.shutdown().await.unwrap();
            });
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            for part in wire.chunks(7) {
                stream.write_all(part).await.unwrap();
                tokio::task::yield_now().await;
            }
            stream.shutdown().await.unwrap();
            let mut result = String::new();
            stream.read_to_string(&mut result).await.unwrap();
            assert_eq!(result, domain);
            peer.await.unwrap();
        }
        for (domain, sni) in [("unknown.example", true), ("a.test", false)] {
            let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
            stream.write_all(&client_hello(domain, sni)).await.unwrap();
            let mut byte = [0];
            let result = stream.read(&mut byte).await;
            assert!(matches!(result, Ok(0) | Err(_)));
        }
        manager.stop("1", None).await;
        assert_eq!(manager.0.lock().await.len(), 1);
        manager.stop("2", None).await;
        let _rebind = TcpListener::bind(("0.0.0.0", port)).await.unwrap();
    })
    .await
    .unwrap();
}

#[test]
fn rejects_missing_duplicate_and_conflicting_host_headers() {
    for request in [
        Request::get("/").body(()).unwrap(),
        Request::get("/")
            .header("Host", "a.test")
            .header("Host", "b.test")
            .body(())
            .unwrap(),
        Request::get("http://b.test/")
            .header("Host", "a.test")
            .body(())
            .unwrap(),
        Request::get("/")
            .header("Host", "user@a.test")
            .body(())
            .unwrap(),
        Request::get("/").header("Host", "*.test").body(()).unwrap(),
    ] {
        assert!(request_domain(&request).is_err());
    }
    assert_eq!(
        request_domain(
            &Request::get("http://a.test:80/path")
                .header("Host", "a.test:80")
                .body(())
                .unwrap()
        )
        .unwrap(),
        "a.test"
    );
}

#[tokio::test]
async fn malformed_http_and_tls_are_rejected_without_a_backend() {
    tokio::time::timeout(Duration::from_secs(15), async {
        for tls in [false, true] {
            let socket = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = socket.local_addr().unwrap();
            let task = tokio::spawn(async move {
                let (stream, peer) = socket.accept().await.unwrap();
                if tls {
                    serve_tls(stream, peer, Arc::default()).await
                } else {
                    serve_http(stream, peer, Arc::default()).await
                }
            });
            let mut stream = TcpStream::connect(addr).await.unwrap();
            stream
                .write_all(
                    b"GET / HTTP/1.1\r\nHost: a.test\r\nHost: b.test\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
            let mut received = Vec::new();
            let _ = stream.read_to_end(&mut received).await;
            if !tls {
                assert!(String::from_utf8_lossy(&received).starts_with("HTTP/1.1 400"));
            } else {
                assert!(received.is_empty());
            }
            let _ = task.await.unwrap();
        }
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn listener_manager_switches_tcp_and_shared_http_without_port_races() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let (provider, traffic, limiter) = fixture();
        let manager = super::super::proxy_server::ProxyListenerManager::new(traffic, limiter);
        let port = port().await;
        let (backend, task) = backend("B").await;
        manager
            .start_client_proxies_from_configs(
                "1".into(),
                vec![config(1, port, backend, "tcp", "")],
                provider.clone(),
            )
            .await
            .unwrap();
        manager.stop_single_proxy("1", 1).await;
        for (id, domain) in [(1, "a.test"), (2, "b.test")] {
            manager
                .start_client_proxies_from_configs(
                    id.to_string(),
                    vec![config(id, port, backend, "http", domain)],
                    provider.clone(),
                )
                .await
                .unwrap();
        }
        manager.stop_client_proxies("1").await;
        let mut stream = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        stream
            .write_all(b"GET / HTTP/1.1\r\nHost: b.test\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut received = String::new();
        stream.read_to_string(&mut received).await.unwrap();
        assert!(received.contains("B:b.test:"));
        manager.stop_client_proxies("2").await;
        manager
            .start_client_proxies_from_configs(
                "1".into(),
                vec![config(1, port, backend, "tcp", "")],
                provider,
            )
            .await
            .unwrap();
        manager.stop_client_proxies("1").await;
        let _rebind = TcpListener::bind(("0.0.0.0", port)).await.unwrap();
        task.abort();
    })
    .await
    .unwrap();
}
