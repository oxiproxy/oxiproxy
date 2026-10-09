use super::*;
use common::protocol::auth::{ClientAuthProvider, TrafficLimitResponse, ValidateTokenResponse};
use common::protocol::control::ProxyControl;
use tokio::io::AsyncWriteExt;

struct TestAuth {
    allowed: bool,
    unavailable: bool,
}
#[async_trait::async_trait]
impl ClientAuthProvider for TestAuth {
    async fn validate_token(&self, _: &str) -> Result<ValidateTokenResponse> {
        bail!("no Client")
    }
    async fn set_client_online(&self, _: i64, _: bool) -> Result<()> {
        bail!("no Client")
    }
    async fn check_traffic_limit(&self, _: i64) -> Result<TrafficLimitResponse> {
        bail!("no Client")
    }
    async fn get_client_proxies(&self, _: i64) -> Result<Vec<ProxyConfig>> {
        bail!("no Client")
    }
    async fn check_direct_proxy_limit(&self, _: i64) -> Result<TrafficLimitResponse> {
        ensure!(!self.unavailable, "Controller unavailable");
        Ok(TrafficLimitResponse {
            exceeded: !self.allowed,
            reason: None,
        })
    }
}

fn direct_config(id: i64, port: u16, kind: &str, domain: &str, origin: String) -> ProxyConfig {
    let mut config = config(id, port, 0, kind, domain);
    config.client_id.clear();
    config.local_ip.clear();
    config.upstream_url = origin;
    config.user_id = Some(7);
    config
}
fn direct_provider(allowed: bool, unavailable: bool) -> ConnectionProvider {
    ConnectionProvider::new(Arc::default(), Arc::default()).with_auth_provider(Arc::new(TestAuth {
        allowed,
        unavailable,
    }))
}
async fn request(port: u16, host: &str) -> Response<Incoming> {
    let socket = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = connection.await;
    });
    sender
        .send_request(
            Request::get("/")
                .header("host", host)
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn direct_http_without_any_client_preserves_requests_upgrades_and_reports_traffic() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_port = listener.local_addr().unwrap().port();
        let backend_task = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let service = hyper::service::service_fn(|req: Request<Incoming>| async move {
                assert_eq!(req.uri(), "/api?q=hello");
                assert_eq!(req.headers()["x-forwarded-host"], "app.test:80");
                assert_eq!(req.headers()["x-forwarded-for"], "127.0.0.1");
                assert_eq!(req.headers()["x-forwarded-proto"], "http");
                assert!(!req.headers().contains_key("forwarded"));
                let host = req.headers()[header::HOST].to_str().unwrap().to_owned();
                let data = req.into_body().collect().await.unwrap().to_bytes();
                Ok::<_, Infallible>(Response::new(Full::new(Bytes::from(format!(
                    "{host}:{}",
                    String::from_utf8_lossy(&data)
                )))))
            });
            hyper::server::conn::http1::Builder::new()
                .serve_connection(TokioIo::new(socket), service)
                .await
                .unwrap();
        });
        let (ws_port, ws_task) = backend("WS").await;
        let manager = DomainListeners::default();
        let (tx, mut rx) = tokio::sync::mpsc::channel(100);
        let traffic = Arc::new(TrafficManager::new(
            super::super::super::grpc_client::SharedGrpcSender::new(tx),
        ));
        let provider = direct_provider(true, false);
        assert!(!provider.is_online("1").await);
        let port = port().await;
        for config in [
            direct_config(
                1,
                port,
                "http",
                "*.test",
                format!("http://127.0.0.1:{backend_port}"),
            ),
            direct_config(
                2,
                port,
                "http",
                "ws.test",
                format!("http://localhost:{ws_port}"),
            ),
            config(3, port, ws_port, "http", "offline.test"),
        ] {
            manager
                .start(
                    config,
                    provider.clone(),
                    traffic.clone(),
                    SpeedLimiter::new(0),
                )
                .await
                .unwrap();
        }
        let socket = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(socket))
            .await
            .unwrap();
        tokio::spawn(async move {
            let _ = connection.with_upgrades().await;
        });
        let result = sender
            .send_request(
                Request::post("/api?q=hello")
                    .header("host", "app.test:80")
                    .header("x-forwarded-for", "spoofed")
                    .header("x-forwarded-host", "spoofed")
                    .header("forwarded", "for=spoofed")
                    .body(Full::new(Bytes::from_static(b"payload")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(result.status(), StatusCode::OK);
        assert_eq!(
            result.into_body().collect().await.unwrap().to_bytes(),
            format!("127.0.0.1:{backend_port}:payload")
        );
        backend_task.await.unwrap();
        assert_eq!(
            request(port, "offline.test").await.status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        let mut response = sender
            .send_request(
                Request::get("/ws")
                    .header("host", "ws.test")
                    .header("connection", "upgrade")
                    .header("upgrade", "websocket")
                    .body(Full::new(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
        let mut websocket = TokioIo::new(hyper::upgrade::on(&mut response).await.unwrap());
        websocket.write_all(b"ping").await.unwrap();
        let mut echo = [0; 4];
        websocket.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"ping");
        drop(websocket);
        let report = rx.recv().await.unwrap();
        let common::grpc::oxiproxy::agent_server_message::Payload::TrafficReport(report) =
            report.payload.unwrap()
        else {
            panic!("expected traffic report")
        };
        let record = report.records.iter().find(|r| r.proxy_id == 1).unwrap();
        assert_eq!(record.client_id, "0");
        assert_eq!(record.user_id, Some(7));
        assert!(record.bytes_sent > 0 && record.bytes_received > 0);
        manager.stop("", None).await;
        assert_eq!(
            request(port, "ws.test").await.status(),
            StatusCode::NOT_FOUND
        );
        manager.stop("3", None).await;
        assert!(manager.0.lock().await.is_empty());
        ws_task.abort();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn direct_http_fails_closed_for_quota_checks_and_unreachable_upstreams() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let manager = DomainListeners::default();
        let (_, traffic, limiter) = fixture();
        let port = port().await;
        let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let unused = reserved.local_addr().unwrap().port();
        drop(reserved);
        for (allowed, unavailable, status) in [
            (false, false, StatusCode::FORBIDDEN),
            (true, true, StatusCode::SERVICE_UNAVAILABLE),
            (true, false, StatusCode::BAD_GATEWAY),
        ] {
            manager
                .start(
                    direct_config(
                        1,
                        port,
                        "http",
                        "app.test",
                        format!("http://127.0.0.1:{unused}"),
                    ),
                    direct_provider(allowed, unavailable),
                    traffic.clone(),
                    limiter.clone(),
                )
                .await
                .unwrap();
            assert_eq!(request(port, "app.test").await.status(), status);
        }
    })
    .await
    .unwrap();
}

fn tls_configs() -> (rustls::ServerConfig, rustls::ClientConfig) {
    let cert = rcgen::generate_simple_self_signed(vec!["app.test".into()]).unwrap();
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der());
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(vec![cert.cert.der().clone()], key.into())
        .unwrap();
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.cert.der().clone()).unwrap();
    let client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_root_certificates(roots)
        .with_no_client_auth();
    (server, client)
}

#[tokio::test]
async fn direct_https_passthrough_leaves_sni_and_certificate_to_the_target() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let manager = DomainListeners::default();
        let (_, traffic, limiter) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_port = listener.local_addr().unwrap().port();
        let (server, client) = tls_configs();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut tls = tokio_rustls::TlsAcceptor::from(Arc::new(server))
                .accept(socket)
                .await
                .unwrap();
            assert_eq!(tls.get_ref().1.server_name(), Some("app.test"));
            let mut data = [0; 4];
            tls.read_exact(&mut data).await.unwrap();
            tls.write_all(&data).await.unwrap();
            tls.shutdown().await.unwrap();
        });
        let port = port().await;
        manager
            .start(
                direct_config(
                    1,
                    port,
                    "https",
                    "app.test",
                    format!("https://127.0.0.1:{backend_port}"),
                ),
                direct_provider(true, false),
                traffic,
                limiter,
            )
            .await
            .unwrap();
        let socket = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
        let mut tls = tokio_rustls::TlsConnector::from(Arc::new(client))
            .connect("app.test".try_into().unwrap(), socket)
            .await
            .unwrap();
        tls.write_all(b"ping").await.unwrap();
        let mut echo = [0; 4];
        tls.read_exact(&mut echo).await.unwrap();
        assert_eq!(&echo, b"ping");
        tls.shutdown().await.unwrap();
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn direct_http_to_https_rejects_an_untrusted_target_certificate() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let manager = DomainListeners::default();
        let (_, traffic, limiter) = fixture();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_port = listener.local_addr().unwrap().port();
        let (server, _) = tls_configs();
        let peer = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            assert!(tokio_rustls::TlsAcceptor::from(Arc::new(server))
                .accept(socket)
                .await
                .is_err());
        });
        let port = port().await;
        manager
            .start(
                direct_config(
                    1,
                    port,
                    "http",
                    "app.test",
                    format!("https://localhost:{backend_port}"),
                ),
                direct_provider(true, false),
                traffic,
                limiter,
            )
            .await
            .unwrap();
        assert_eq!(
            request(port, "app.test").await.status(),
            StatusCode::BAD_GATEWAY
        );
        peer.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn direct_snapshot_restores_updates_and_removes_routes_without_a_client() {
    use crate::server::{
        config_manager::ConfigManager, local_proxy_control::LocalProxyControl,
        proxy_server::ProxyServer,
    };
    tokio::time::timeout(Duration::from_secs(5), async {
        let (_, traffic, limiter) = fixture();
        let server = Arc::new(
            ProxyServer::new(
                traffic,
                Arc::new(ConfigManager::new()),
                Arc::new(TestAuth {
                    allowed: true,
                    unavailable: false,
                }),
                limiter,
                None,
                None,
            )
            .unwrap(),
        );
        let control = LocalProxyControl::new(
            server.get_listener_manager(),
            server.get_client_connections(),
            server.get_tunnel_connections(),
            Arc::new(TestAuth {
                allowed: true,
                unavailable: false,
            }),
            server,
        );
        let (a, task_a) = backend("A").await;
        let (b, task_b) = backend("B").await;
        let port = port().await;
        let mut config =
            direct_config(1, port, "http", "app.test", format!("http://127.0.0.1:{a}"));
        control
            .sync_direct_proxies(vec![config.clone()])
            .await
            .unwrap();
        assert_eq!(
            control
                .get_server_status()
                .await
                .unwrap()
                .active_proxy_count,
            1
        );
        assert_eq!(
            request(port, "app.test")
                .await
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes(),
            format!("A:127.0.0.1:{a}:")
        );
        config.upstream_url = format!("http://127.0.0.1:{b}");
        control.start_direct_proxy(config).await.unwrap();
        assert_eq!(
            request(port, "app.test")
                .await
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes(),
            format!("B:127.0.0.1:{b}:")
        );
        control.sync_direct_proxies(vec![]).await.unwrap();
        assert_eq!(
            control
                .get_server_status()
                .await
                .unwrap()
                .active_proxy_count,
            0
        );
        assert!(TcpListener::bind(("0.0.0.0", port)).await.is_ok());
        task_a.abort();
        task_b.abort();
    })
    .await
    .unwrap();
}
