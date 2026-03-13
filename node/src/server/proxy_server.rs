use anyhow::Result;
use quinn::{Endpoint, ServerConfig, TransportConfig, VarInt};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tracing::{info, warn, error, debug};
use serde::{Serialize, Deserialize};

use crate::server::traffic::TrafficManager;
use crate::server::config_manager::ConfigManager;
use common::KcpConfig;

// 从共享库导入隧道模块
use common::{
    TunnelConnection, TunnelSendStream, TunnelRecvStream,
    TunnelListener, KcpListener, TcpTunnelListener, QuicSendStream, QuicRecvStream
};
use common::utils::create_configured_udp_socket;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ProxyProtocol {
    Tcp,
    Udp,
}

impl From<String> for ProxyProtocol {
    fn from(s: String) -> Self {
        match s.to_lowercase().as_str() {
            "udp" => ProxyProtocol::Udp,
            _ => ProxyProtocol::Tcp,
        }
    }
}

impl From<&str> for ProxyProtocol {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "udp" => ProxyProtocol::Udp,
            _ => ProxyProtocol::Tcp,
        }
    }
}

impl ProxyProtocol {
    pub fn as_str(&self) -> &str {
        match self {
            ProxyProtocol::Tcp => "tcp",
            ProxyProtocol::Udp => "udp",
        }
    }
}

// UDP会话信息
#[allow(dead_code)]
struct UdpSession {
    target_addr: SocketAddr,
    last_activity: tokio::time::Instant,
}

pub struct ProxyServer {
    cert: CertificateDer<'static>,
    key: PrivateKeyDer<'static>,
    traffic_manager: Arc<TrafficManager>,
    listener_manager: Arc<ProxyListenerManager>,
    client_connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
    tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
    config_manager: Arc<ConfigManager>,
    auth_provider: Arc<dyn common::protocol::auth::ClientAuthProvider>,
}

/// Unified connection type that can be either QUIC or KCP
#[derive(Clone)]
pub enum UnifiedConnection {
    Quic(Arc<quinn::Connection>),
    Tunnel(Arc<Box<dyn TunnelConnection>>),
}

impl UnifiedConnection {
    pub async fn open_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        match self {
            UnifiedConnection::Quic(conn) => {
                let (send, recv) = conn.open_bi().await?;
                Ok((
                    Box::new(QuicSendStream::new(send)) as Box<dyn TunnelSendStream>,
                    Box::new(QuicRecvStream::new(recv)) as Box<dyn TunnelRecvStream>,
                ))
            }
            UnifiedConnection::Tunnel(conn) => {
                conn.open_bi().await
            }
        }
    }
}

// 代理监听器管理器
pub struct ProxyListenerManager {
    // client_id -> (proxy_id, JoinHandle)
    listeners: Arc<RwLock<HashMap<String, HashMap<i64, JoinHandle<()>>>>>,
    // UDP会话管理: (client_id, proxy_id) -> (source_addr -> UdpSession)
    udp_sessions: Arc<RwLock<HashMap<(String, i64), HashMap<SocketAddr, UdpSession>>>>,
    traffic_manager: Arc<TrafficManager>,
    speed_limiter: Arc<super::speed_limiter::SpeedLimiter>,
}

/// Connection provider for proxy listeners
#[derive(Clone)]
pub struct ConnectionProvider {
    quic_connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
    tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
}

impl ConnectionProvider {
    pub fn new(
        quic_connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
        tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
    ) -> Self {
        Self {
            quic_connections,
            tunnel_connections,
        }
    }

    /// Get a unified connection for a client
    pub async fn get_connection(&self, client_id: &str) -> Option<UnifiedConnection> {
        // First check QUIC connections
        {
            let quic_conns = self.quic_connections.read().await;
            if let Some(conn) = quic_conns.get(client_id) {
                return Some(UnifiedConnection::Quic(conn.clone()));
            }
        }
        // Then check tunnel (KCP) connections
        {
            let tunnel_conns = self.tunnel_connections.read().await;
            if let Some(conn) = tunnel_conns.get(client_id) {
                return Some(UnifiedConnection::Tunnel(conn.clone()));
            }
        }
        None
    }

    /// Check if a client is online
    pub async fn is_online(&self, client_id: &str) -> bool {
        self.get_connection(client_id).await.is_some()
    }
}

impl ProxyListenerManager {
    pub fn new(traffic_manager: Arc<TrafficManager>, speed_limiter: Arc<super::speed_limiter::SpeedLimiter>) -> Self {
        Self {
            listeners: Arc::new(RwLock::new(HashMap::new())),
            udp_sessions: Arc::new(RwLock::new(HashMap::new())),
            traffic_manager,
            speed_limiter,
        }
    }

    // 从代理配置列表启动代理监听器
    pub async fn start_client_proxies_from_configs(
        &self,
        client_id: String,
        proxies: Vec<common::protocol::control::ProxyConfig>,
        conn_provider: ConnectionProvider,
    ) -> Result<()> {
        if proxies.is_empty() {
            info!("  [客户端 {}] 没有启用的代理", client_id);
            return Ok(());
        }

        let mut listeners = self.listeners.write().await;
        let client_listeners = listeners.entry(client_id.clone()).or_insert_with(HashMap::new);

        for proxy in proxies {
            // 如果该代理的监听器已经运行，跳过
            if client_listeners.contains_key(&proxy.proxy_id) {
                continue;
            }

            let proxy_name = proxy.name.clone();
            let proxy_protocol: ProxyProtocol = proxy.proxy_type.clone().into();
            let proxy_protocol_str = proxy_protocol.as_str().to_uppercase();
            let client_id_clone = client_id.clone();
            let listen_addr = format!("0.0.0.0:{}", proxy.remote_port);
            let target_addr = format!("{}:{}", proxy.local_ip, proxy.local_port);
            let proxy_id = proxy.proxy_id;
            let conn_provider_clone = conn_provider.clone();
            let traffic_manager = self.traffic_manager.clone();

            // 预检端口是否可用：尝试绑定后立即释放
            match proxy_protocol {
                ProxyProtocol::Tcp => {
                    match TcpListener::bind(&listen_addr).await {
                        Ok(_listener) => {
                            // 绑定成功，drop 释放端口，后续 spawn 任务会重新绑定
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "代理「{}」无法监听 {} 端口 {}：{}",
                                proxy_name, proxy_protocol_str, proxy.remote_port, e
                            ));
                        }
                    }
                }
                ProxyProtocol::Udp => {
                    match UdpSocket::bind(&listen_addr).await {
                        Ok(_socket) => {
                            // 绑定成功，drop 释放端口
                        }
                        Err(e) => {
                            return Err(anyhow::anyhow!(
                                "代理「{}」无法监听 {} 端口 {}：{}",
                                proxy_name, proxy_protocol_str, proxy.remote_port, e
                            ));
                        }
                    }
                }
            }

            let udp_sessions = self.udp_sessions.clone();
            let speed_limiter = self.speed_limiter.clone();

            let handle = tokio::spawn(async move {
                loop {
                    let result = match proxy_protocol {
                        ProxyProtocol::Tcp => {
                            run_tcp_proxy_listener_unified(
                                proxy_name.clone(),
                                client_id_clone.clone(),
                                listen_addr.clone(),
                                target_addr.clone(),
                                conn_provider_clone.clone(),
                                proxy_id,
                                traffic_manager.clone(),
                                speed_limiter.clone(),
                            ).await
                        }
                        ProxyProtocol::Udp => {
                            run_udp_proxy_listener_unified(
                                proxy_name.clone(),
                                client_id_clone.clone(),
                                listen_addr.clone(),
                                target_addr.clone(),
                                conn_provider_clone.clone(),
                                proxy_id,
                                udp_sessions.clone(),
                                traffic_manager.clone(),
                                speed_limiter.clone(),
                            ).await
                        }
                    };

                    match result {
                        Ok(_) => {},
                        Err(e) => {
                            error!("[{}] 代理监听失败: {}", proxy_name, e);
                        }
                    }
                    // 如果监听器失败，等待一段时间后重新尝试启动（如果客户端仍在线）
                    tokio::time::sleep(Duration::from_secs(5)).await;

                    // 检查客户端是否仍在连接
                    if !conn_provider_clone.is_online(&client_id_clone).await {
                        warn!("[{}] 客户端已离线，停止代理监听", proxy_name);
                        break;
                    }
                }
            });

            client_listeners.insert(proxy_id, handle);
            info!("  [客户端 {}] 启动{}代理: {} 端口: {}",
                  client_id, proxy_protocol_str, proxy.name, proxy.remote_port);
        }

        Ok(())
    }

    // 停止客户端的所有代理监听器
    pub async fn stop_client_proxies(&self, client_id: &str) {
        let mut listeners = self.listeners.write().await;
        if let Some(client_listeners) = listeners.remove(client_id) {
            info!("  [客户端 {}] 停止 {} 个代理监听器", client_id, client_listeners.len());
            for (proxy_id, handle) in client_listeners {
                handle.abort();
                debug!("    代理 #{} 已停止", proxy_id);
            }
        }
    }

    // 停止单个代理监听器（用于删除或禁用代理时）
    pub async fn stop_single_proxy(&self, client_id: &str, proxy_id: i64) {
        let mut listeners = self.listeners.write().await;
        if let Some(client_listeners) = listeners.get_mut(client_id) {
            if let Some(handle) = client_listeners.remove(&proxy_id) {
                handle.abort();
                info!("  [客户端 {}] 停止代理 #{}", client_id, proxy_id);
            }
        }
    }
}

impl ProxyServer {
    pub fn new(
        traffic_manager: Arc<TrafficManager>,
        config_manager: Arc<ConfigManager>,
        auth_provider: Arc<dyn common::protocol::auth::ClientAuthProvider>,
        speed_limiter: Arc<super::speed_limiter::SpeedLimiter>,
    ) -> Result<Self> {
        let cert = rcgen::generate_simple_self_signed(&["oxiproxy".to_string()])?;
        let listener_manager = Arc::new(ProxyListenerManager::new(traffic_manager.clone(), speed_limiter));
        let client_connections = Arc::new(RwLock::new(HashMap::new()));
        let tunnel_connections = Arc::new(RwLock::new(HashMap::new()));

        Ok(Self {
            cert: CertificateDer::from(cert.cert.der().to_vec()),
            key: PrivateKeyDer::from(PrivatePkcs8KeyDer::from(cert.signing_key.serialize_der())),
            traffic_manager,
            listener_manager,
            client_connections,
            tunnel_connections,
            config_manager,
            auth_provider,
        })
    }

    pub fn get_listener_manager(&self) -> Arc<ProxyListenerManager> {
        self.listener_manager.clone()
    }

    pub fn get_client_connections(&self) -> Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>> {
        self.client_connections.clone()
    }

    pub fn get_tunnel_connections(&self) -> Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>> {
        self.tunnel_connections.clone()
    }

    /// Get a unified connection for a client (checks both QUIC and KCP)
    pub async fn get_unified_connection(&self, client_id: &str) -> Option<UnifiedConnection> {
        // First check QUIC connections
        {
            let quic_conns = self.client_connections.read().await;
            if let Some(conn) = quic_conns.get(client_id) {
                return Some(UnifiedConnection::Quic(conn.clone()));
            }
        }
        // Then check tunnel (KCP) connections
        {
            let tunnel_conns = self.tunnel_connections.read().await;
            if let Some(conn) = tunnel_conns.get(client_id) {
                return Some(UnifiedConnection::Tunnel(conn.clone()));
            }
        }
        None
    }

    /// Check if a client is online (either QUIC or KCP)
    pub async fn is_client_online(&self, client_id: &str) -> bool {
        let quic_online = {
            let conns = self.client_connections.read().await;
            conns.contains_key(client_id)
        };
        if quic_online {
            return true;
        }
        let tunnel_online = {
            let conns = self.tunnel_connections.read().await;
            conns.contains_key(client_id)
        };
        tunnel_online
    }

    pub async fn run(&self, bind_addr: String) -> Result<()> {
        // 从配置管理器获取配置
        let idle_timeout = self.config_manager.get_number("idle_timeout", 60).await as u64;
        let max_streams = self.config_manager.get_number("max_concurrent_streams", 100).await as u32;
        let keep_alive_interval = self.config_manager.get_number("keep_alive_interval", 5).await as u64;

        let mut transport_config = TransportConfig::default();
        transport_config.max_concurrent_uni_streams(VarInt::from_u32(max_streams));
        // 服务器也发送心跳，确保连接稳定
        transport_config.keep_alive_interval(Some(Duration::from_secs(keep_alive_interval)));
        transport_config.max_idle_timeout(Some(Duration::from_secs(idle_timeout).try_into()?));

        let mut server_config = ServerConfig::with_single_cert(
            vec![self.cert.clone()],
            self.key.clone_key(),
        )?;
        server_config.transport_config(Arc::new(transport_config));

        let endpoint = Endpoint::server(server_config, bind_addr.parse()?)?;

        info!("🚀 QUIC服务器启动成功!");
        info!("📡 监听地址: {}", bind_addr);
        info!("⏱️  空闲超时: {}秒 (心跳由客户端主动发送)", idle_timeout);
        info!("🔢 最大并发流: {}", max_streams);

        info!("⏳ 等待客户端连接...");

        // 接受客户端连接
        let mut consecutive_errors: u32 = 0;
        while let Some(connecting) = endpoint.accept().await {
            match connecting.await {
                Ok(conn) => {
                    consecutive_errors = 0;
                    let remote_addr = conn.remote_address();
                    info!("📡 新连接来自: {}", remote_addr);

                    // 等待客户端发送 token 认证
                    let conn_clone = Arc::new(conn);
                    let connections = self.client_connections.clone();
                    let tunnel_connections = self.tunnel_connections.clone();
                    let listener_mgr = self.listener_manager.clone();
                    let config_mgr = self.config_manager.clone();
                    let auth_provider = self.auth_provider.clone();

                    tokio::spawn(async move {
                        debug!("开始处理连接！");
                        if let Err(e) = handle_client_auth(conn_clone, connections, tunnel_connections, listener_mgr, config_mgr, auth_provider).await {
                            error!("❌ 客户端认证失败: {}", e);
                        }
                    });
                }
                Err(e) => {
                    consecutive_errors = consecutive_errors.saturating_add(1);
                    if consecutive_errors <= 3 {
                        error!("❌ 连接接受失败: {}", e);
                    } else if consecutive_errors % 100 == 0 {
                        error!("❌ 连接接受持续失败 (已连续 {} 次): {}", consecutive_errors, e);
                    }
                    let backoff = Duration::from_millis(100 * (consecutive_errors.min(50) as u64));
                    tokio::time::sleep(backoff).await;
                }
            }
        }

        Ok(())
    }

    /// Run KCP server on specified address
    pub async fn run_kcp(&self, bind_addr: String, kcp_config: Option<KcpConfig>) -> Result<()> {
        let addr: SocketAddr = bind_addr.parse()?;
        let listener = KcpListener::new(addr, kcp_config).await?;

        info!("KCP server started successfully!");
        info!("KCP listening on: {}", bind_addr);
        info!("Waiting for KCP client connections...");

        let mut consecutive_errors: u32 = 0;
        loop {
            match listener.accept().await {
                Ok(conn) => {
                    consecutive_errors = 0;
                    let remote_addr = conn.remote_address();
                    info!("New KCP connection from: {}", remote_addr);

                    let conn = Arc::new(conn);
                    let tunnel_connections = self.tunnel_connections.clone();
                    let listener_mgr = self.listener_manager.clone();
                    let config_mgr = self.config_manager.clone();
                    let quic_connections = self.client_connections.clone();
                    let auth_provider = self.auth_provider.clone();

                    tokio::spawn(async move {
                        debug!("Processing KCP connection!");
                        if let Err(e) = handle_tunnel_client_auth(
                            conn,
                            tunnel_connections,
                            quic_connections,
                            listener_mgr,
                            config_mgr,
                            auth_provider,
                        ).await {
                            error!("KCP client authentication failed: {}", e);
                        }
                    });
                }
                Err(e) => {
                    consecutive_errors = consecutive_errors.saturating_add(1);
                    if consecutive_errors <= 3 {
                        error!("KCP connection accept failed: {}", e);
                    } else if consecutive_errors % 100 == 0 {
                        error!("KCP connection accept keeps failing ({} times): {}", consecutive_errors, e);
                    }
                    let backoff = Duration::from_millis(100 * (consecutive_errors.min(50) as u64));
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }

    /// Run TCP+yamux server on specified address
    pub async fn run_tcp(&self, bind_addr: String) -> Result<()> {
        let addr: SocketAddr = bind_addr.parse()?;
        let listener = TcpTunnelListener::new(addr).await?;

        info!("TCP tunnel server started successfully!");
        info!("TCP listening on: {}", bind_addr);
        info!("Waiting for TCP client connections...");

        let mut consecutive_errors: u32 = 0;
        loop {
            match listener.accept().await {
                Ok(conn) => {
                    consecutive_errors = 0;
                    let remote_addr = conn.remote_address();
                    info!("New TCP tunnel connection from: {}", remote_addr);

                    let conn = Arc::new(conn);
                    let tunnel_connections = self.tunnel_connections.clone();
                    let listener_mgr = self.listener_manager.clone();
                    let config_mgr = self.config_manager.clone();
                    let quic_connections = self.client_connections.clone();
                    let auth_provider = self.auth_provider.clone();

                    tokio::spawn(async move {
                        debug!("Processing TCP tunnel connection!");
                        if let Err(e) = handle_tunnel_client_auth(
                            conn,
                            tunnel_connections,
                            quic_connections,
                            listener_mgr,
                            config_mgr,
                            auth_provider,
                        ).await {
                            error!("TCP tunnel client authentication failed: {}", e);
                        }
                    });
                }
                Err(e) => {
                    consecutive_errors = consecutive_errors.saturating_add(1);
                    if consecutive_errors <= 3 {
                        error!("TCP tunnel connection accept failed: {}", e);
                    } else if consecutive_errors % 100 == 0 {
                        error!("TCP tunnel connection accept keeps failing ({} times): {}", consecutive_errors, e);
                    }
                    let backoff = Duration::from_millis(100 * (consecutive_errors.min(50) as u64));
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
}

async fn handle_client_auth(
    conn: Arc<quinn::Connection>,
    connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
    tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
    listener_manager: Arc<ProxyListenerManager>,
    config_manager: Arc<ConfigManager>,
    auth_provider: Arc<dyn common::protocol::auth::ClientAuthProvider>,
) -> Result<()> {
    // 等待客户端发送 token (格式: 2字节长度 + 内容)
    let mut recv_stream = match conn.accept_uni().await {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut len_buf = [0u8; 2];
    recv_stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    debug!("接收token长度: {}", len);

    let mut token_buf = vec![0u8; len];
    recv_stream.read_exact(&mut token_buf).await?;
    let token = String::from_utf8(token_buf)?;
    debug!("接收token: {}", token);

    // 通过 auth_provider 验证 token
    let auth_result = auth_provider.validate_token(&token).await?;
    if !auth_result.allowed {
        error!("❌ 客户端认证失败: {}", auth_result.reject_reason.unwrap_or_default());
        return Ok(());
    }

    let client_id = auth_result.client_id;
    let client_name = auth_result.client_name;

    // 更新客户端为在线状态
    if let Err(e) = auth_provider.set_client_online(client_id, true).await {
        error!("❌ 更新客户端在线状态失败: {}", e);
    }

    info!("✅ 客户端认证成功: {} (ID: {}, 在线: {})", client_name, client_id, conn.remote_address());

    // 保存连接（先保存，再启动代理，这样代理监听器能找到连接）
    let mut conns = connections.write().await;
    conns.insert(format!("{}", client_id), conn.clone());
    drop(conns);

    // 启动该客户端的所有代理监听器（使用统一连接提供器）
    let conn_provider = ConnectionProvider::new(connections.clone(), tunnel_connections.clone());
    // 从 auth_provider 获取代理配置（兼容本地和远程模式）
    match auth_provider.get_client_proxies(client_id).await {
        Ok(proxies) => {
            if let Err(e) = listener_manager.start_client_proxies_from_configs(format!("{}", client_id), proxies, conn_provider).await {
                error!("❌ 启动代理监听器失败: {}", e);
            }
        }
        Err(e) => {
            error!("❌ 获取代理配置失败: {}", e);
        }
    }

    // 启动连接健康检查任务
    let conn_health_check = conn.clone();
    let client_id_health = client_id;
    let client_name_health = client_name.clone();
    let connections_health = connections.clone();
    let listener_manager_health = listener_manager.clone();
    let auth_provider_health = auth_provider.clone();

    // 从配置获取健康检查间隔
    let health_check_interval = config_manager.get_number("health_check_interval", 15).await as u64;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(health_check_interval));
        loop {
            interval.tick().await;

            // 检查连接是否仍然有效
            if conn_health_check.close_reason().is_some() {
                warn!("⚠️  检测到客户端连接已关闭: {}", client_name_health);

                // 清理连接 - 只有当前连接与健康检查的连接是同一个时才删除
                let client_id_str = format!("{}", client_id_health);
                let mut conns = connections_health.write().await;

                // 检查当前存储的连接是否与我们监控的连接是同一个
                let should_cleanup = if let Some(current_conn) = conns.get(&client_id_str) {
                    // 比较连接的远程地址来判断是否是同一个连接
                    Arc::ptr_eq(current_conn, &conn_health_check)
                } else {
                    false
                };

                if should_cleanup {
                    conns.remove(&client_id_str);
                    drop(conns);

                    // 停止该客户端的所有代理监听器
                    listener_manager_health.stop_client_proxies(&client_id_str).await;

                    // 更新客户端为离线状态
                    if let Err(e) = auth_provider_health.set_client_online(client_id_health, false).await {
                        error!("更新客户端离线状态失败: {}", e);
                    }
                } else {
                    drop(conns);
                    debug!("跳过清理: 客户端 {} 已经重新连接", client_name_health);
                }
                break;
            }
        }
    });

    // 循环接受代理流请求
    loop {
        match conn.accept_bi().await {
            Ok((send, recv)) => {
                let conn_clone = conn.clone();
                let connections_clone = connections.clone();

                tokio::spawn(async move {
                    // 先读取消息类型
                    let mut msg_type = [0u8; 1];
                    let mut recv = recv;
                    if recv.read_exact(&mut msg_type).await.is_err() {
                        return;
                    }

                    match msg_type[0] {
                        b'h' => {
                            // 心跳请求，回复心跳
                            if let Err(e) = handle_heartbeat(send).await {
                                debug!("心跳处理错误: {}", e);
                            }
                        }
                        _ => {
                            // 其他消息类型，交给代理流处理
                            if let Err(e) = handle_proxy_stream(send, recv, conn_clone, connections_clone).await {
                                error!("❌ 处理代理流错误: {}", e);
                            }
                        }
                    }
                });
            }
            Err(_) => {
                warn!("⚠️  客户端断开连接: {}", client_name);
                let client_id_str = format!("{}", client_id);
                let mut conns = connections.write().await;

                // 检查当前存储的连接是否与我们监控的连接是同一个（防止误删重连后的新连接）
                let should_cleanup = if let Some(current_conn) = conns.get(&client_id_str) {
                    Arc::ptr_eq(current_conn, &conn)
                } else {
                    false
                };

                if should_cleanup {
                    conns.remove(&client_id_str);
                    drop(conns);

                    // 停止该客户端的所有代理监听器
                    listener_manager.stop_client_proxies(&client_id_str).await;

                    // 更新客户端为离线状态
                    if let Err(e) = auth_provider.set_client_online(client_id, false).await {
                        error!("更新客户端离线状态失败: {}", e);
                    }
                } else {
                    drop(conns);
                    debug!("跳过清理: 客户端 {} 已经重新连接", client_name);
                }
                break;
            }
        }
    }

    Ok(())
}

/// Handle client authentication for tunnel connections (KCP)
async fn handle_tunnel_client_auth(
    conn: Arc<Box<dyn TunnelConnection>>,
    tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
    quic_connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
    listener_manager: Arc<ProxyListenerManager>,
    config_manager: Arc<ConfigManager>,
    auth_provider: Arc<dyn common::protocol::auth::ClientAuthProvider>,
) -> Result<()> {
    // Wait for client to send token (format: 2 byte length + content)
    let mut recv_stream = match conn.accept_uni().await {
        Ok(s) => s,
        Err(_) => return Ok(()),
    };

    let mut len_buf = [0u8; 2];
    recv_stream.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;
    debug!("Received token length: {}", len);

    let mut token_buf = vec![0u8; len];
    recv_stream.read_exact(&mut token_buf).await?;
    let token = String::from_utf8(token_buf)?;
    debug!("Received token: {}", token);

    // 通过 auth_provider 验证 token
    let auth_result = auth_provider.validate_token(&token).await?;
    if !auth_result.allowed {
        error!("KCP client auth failed: {}", auth_result.reject_reason.unwrap_or_default());
        return Ok(());
    }

    let client_id = auth_result.client_id;
    let client_name = auth_result.client_name;

    // 更新客户端为在线状态
    if let Err(e) = auth_provider.set_client_online(client_id, true).await {
        error!("Failed to update client online status: {}", e);
    }

    info!("KCP client authenticated: {} (ID: {}, Online: {})", client_name, client_id, conn.remote_address());

    // Save tunnel connection first (so proxy listeners can find it)
    let mut conns = tunnel_connections.write().await;
    conns.insert(format!("{}", client_id), conn.clone());
    drop(conns);

    // Start all proxy listeners for this client (using unified connection provider)
    let conn_provider = ConnectionProvider::new(quic_connections.clone(), tunnel_connections.clone());
    // 从 auth_provider 获取代理配置（兼容本地和远程模式）
    match auth_provider.get_client_proxies(client_id).await {
        Ok(proxies) => {
            if let Err(e) = listener_manager.start_client_proxies_from_configs(format!("{}", client_id), proxies, conn_provider).await {
                error!("Failed to start proxy listeners: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to get proxy configs: {}", e);
        }
    }

    // Start connection health check task
    let conn_health_check = conn.clone();
    let client_id_health = client_id;
    let client_name_health = client_name.clone();
    let tunnel_connections_health = tunnel_connections.clone();
    let listener_manager_health = listener_manager.clone();
    let auth_provider_health = auth_provider.clone();

    let health_check_interval = config_manager.get_number("health_check_interval", 15).await as u64;

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(health_check_interval));
        loop {
            interval.tick().await;

            if conn_health_check.close_reason().is_some() {
                warn!("Detected KCP client connection closed: {}", client_name_health);

                let client_id_str = format!("{}", client_id_health);
                let mut conns = tunnel_connections_health.write().await;

                let should_cleanup = if let Some(current_conn) = conns.get(&client_id_str) {
                    Arc::ptr_eq(current_conn, &conn_health_check)
                } else {
                    false
                };

                if should_cleanup {
                    conns.remove(&client_id_str);
                    drop(conns);

                    listener_manager_health.stop_client_proxies(&client_id_str).await;

                    if let Err(e) = auth_provider_health.set_client_online(client_id_health, false).await {
                        error!("Failed to update client offline status: {}", e);
                    }
                } else {
                    drop(conns);
                    debug!("Skipping cleanup: KCP client {} has already reconnected", client_name_health);
                }
                break;
            }
        }
    });

    // Loop to accept proxy stream requests
    loop {
        match conn.accept_bi().await {
            Ok((send, mut recv)) => {
                let conn_clone = conn.clone();
                let tunnel_connections_clone = tunnel_connections.clone();

                tokio::spawn(async move {
                    // Read message type
                    let mut msg_type = [0u8; 1];
                    if recv.read_exact(&mut msg_type).await.is_err() {
                        return;
                    }

                    match msg_type[0] {
                        b'h' => {
                            // Heartbeat request
                            if let Err(e) = handle_tunnel_heartbeat(send).await {
                                debug!("Heartbeat error: {}", e);
                            }
                        }
                        _ => {
                            // Other message types
                            if let Err(e) = handle_tunnel_proxy_stream(send, recv, conn_clone, tunnel_connections_clone).await {
                                error!("Tunnel proxy stream error: {}", e);
                            }
                        }
                    }
                });
            }
            Err(_) => {
                warn!("KCP client disconnected: {}", client_name);
                let client_id_str = format!("{}", client_id);
                let mut conns = tunnel_connections.write().await;

                // 检查当前存储的连接是否与我们监控的连接是同一个（防止误删重连后的新连接）
                let should_cleanup = if let Some(current_conn) = conns.get(&client_id_str) {
                    Arc::ptr_eq(current_conn, &conn)
                } else {
                    false
                };

                if should_cleanup {
                    conns.remove(&client_id_str);
                    drop(conns);

                    listener_manager.stop_client_proxies(&client_id_str).await;

                    if let Err(e) = auth_provider.set_client_online(client_id, false).await {
                        error!("Failed to update client offline status: {}", e);
                    }
                } else {
                    drop(conns);
                    debug!("Skipping cleanup: KCP client {} has already reconnected", client_name);
                }
                break;
            }
        }
    }

    Ok(())
}

/// Handle heartbeat for tunnel connections
async fn handle_tunnel_heartbeat(mut send: Box<dyn TunnelSendStream>) -> Result<()> {
    send.write_all(&[b'h']).await?;
    send.finish().await?;
    Ok(())
}

/// Handle proxy stream for tunnel connections
async fn handle_tunnel_proxy_stream(
    mut tunnel_send: Box<dyn TunnelSendStream>,
    mut tunnel_recv: Box<dyn TunnelRecvStream>,
    _conn: Arc<Box<dyn TunnelConnection>>,
    _connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
) -> Result<()> {
    // Read target address
    let mut len_buf = [0u8; 2];
    tunnel_recv.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut addr_buf = vec![0u8; len];
    tunnel_recv.read_exact(&mut addr_buf).await?;
    let target_addr = String::from_utf8(addr_buf)?;

    // Connect to target service
    let mut tcp_stream = TcpStream::connect(&target_addr).await?;

    let (mut tcp_read, mut tcp_write) = tcp_stream.split();

    // Tunnel -> TCP
    let tunnel_to_tcp = async {
        let mut buf = vec![0u8; 32768];
        loop {
            match tunnel_recv.read(&mut buf).await? {
                Some(n) => {
                    if n == 0 {
                        break;
                    }
                    tcp_write.write_all(&buf[..n]).await?;
                }
                None => break,
            }
        }
        let _ = tcp_write.shutdown().await;
        Ok::<_, anyhow::Error>(())
    };

    // TCP -> Tunnel
    let tcp_to_tunnel = async {
        let mut buf = vec![0u8; 32768];
        loop {
            let n = tcp_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            tunnel_send.write_all(&buf[..n]).await?;
        }
        let _ = tunnel_send.finish().await;
        Ok::<_, anyhow::Error>(())
    };

    let (res1, res2) = tokio::join!(tunnel_to_tcp, tcp_to_tunnel);
    if let Err(e) = res1 {
        error!("Tunnel->TCP error: {}", e);
    }
    if let Err(e) = res2 {
        error!("TCP->Tunnel error: {}", e);
    }

    Ok(())
}

/// 处理心跳请求
async fn handle_heartbeat(mut send: quinn::SendStream) -> Result<()> {
    // 回复心跳 'h'
    send.write_all(&[b'h']).await?;
    send.finish()?;
    Ok(())
}

async fn handle_proxy_stream(
    mut quic_send: quinn::SendStream,
    mut quic_recv: quinn::RecvStream,
    _conn: Arc<quinn::Connection>,
    _connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
) -> Result<()> {
    // 读取目标地址（客户端已连接）
    let mut len_buf = [0u8; 2];
    quic_recv.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut addr_buf = vec![0u8; len];
    quic_recv.read_exact(&mut addr_buf).await?;
    let target_addr = String::from_utf8(addr_buf)?;

    // 连接到目标服务
    let mut tcp_stream = TcpStream::connect(&target_addr).await?;

    let (mut tcp_read, mut tcp_write) = tcp_stream.split();

    // QUIC -> TCP
    let quic_to_tcp = async {
        let mut buf = vec![0u8; 32768];
        loop {
            match quic_recv.read(&mut buf).await? {
                Some(n) => {
                    if n == 0 {
                        break;
                    }
                    tcp_write.write_all(&buf[..n]).await?;
                }
                None => break,
            }
        }
        let _ = tcp_write.shutdown().await;
        Ok::<_, anyhow::Error>(())
    };

    // TCP -> QUIC
    let tcp_to_quic = async {
        let mut buf = vec![0u8; 32768];
        loop {
            let n = tcp_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            quic_send.write_all(&buf[..n]).await?;
        }
        quic_send.finish()?;
        Ok::<_, anyhow::Error>(())
    };

    let (res1, res2) = tokio::join!(quic_to_tcp, tcp_to_quic);
    if let Err(e) = res1 {
        error!("QUIC->TCP错误: {}", e);
    }
    if let Err(e) = res2 {
        error!("TCP->QUIC错误: {}", e);
    }

    Ok(())
}

// ============== 统一版本的代理监听器（支持 QUIC 和 KCP）==============

async fn run_tcp_proxy_listener_unified(
    proxy_name: String,
    client_id: String,
    listen_addr: String,
    target_addr: String,
    conn_provider: ConnectionProvider,
    proxy_id: i64,
    traffic_manager: Arc<TrafficManager>,
    speed_limiter: Arc<super::speed_limiter::SpeedLimiter>,
) -> Result<()> {
    let listener = TcpListener::bind(&listen_addr).await?;
    info!("[{}] 🔌 TCP监听端口: {} -> {}", proxy_name, listen_addr, target_addr);

    let mut consecutive_errors: u32 = 0;
    loop {
        match listener.accept().await {
            Ok((tcp_stream, addr)) => {
                consecutive_errors = 0;
                info!("[{}] 📥 新连接来自: {}", proxy_name, addr);

                let conn_provider_clone = conn_provider.clone();
                let client_id = client_id.clone();
                let target_addr = target_addr.clone();
                let proxy_name = proxy_name.clone();
                let traffic_manager = traffic_manager.clone();
                let speed_limiter = speed_limiter.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_to_tunnel_unified(
                        tcp_stream,
                        addr,
                        target_addr,
                        proxy_name,
                        client_id,
                        conn_provider_clone,
                        proxy_id,
                        traffic_manager,
                        speed_limiter,
                    ).await {
                        error!("❌ 处理连接错误: {}", e);
                    }
                });
            }
            Err(e) => {
                consecutive_errors = consecutive_errors.saturating_add(1);
                if consecutive_errors <= 3 {
                    error!("[{}] ❌ 接受连接失败: {}", proxy_name, e);
                } else if consecutive_errors % 100 == 0 {
                    error!("[{}] ❌ 接受连接持续失败 (已连续 {} 次): {}", proxy_name, consecutive_errors, e);
                }
                let backoff = Duration::from_millis(100 * (consecutive_errors.min(50) as u64));
                tokio::time::sleep(backoff).await;
            }
        }
    }
}

async fn run_udp_proxy_listener_unified(
    proxy_name: String,
    client_id: String,
    listen_addr: String,
    target_addr: String,
    conn_provider: ConnectionProvider,
    proxy_id: i64,
    udp_sessions: Arc<RwLock<HashMap<(String, i64), HashMap<SocketAddr, UdpSession>>>>,
    traffic_manager: Arc<TrafficManager>,
    speed_limiter: Arc<super::speed_limiter::SpeedLimiter>,
) -> Result<()> {
    let socket = Arc::new(create_configured_udp_socket(listen_addr.parse()?).await?);
    info!("[{}] 🔌 UDP监听端口: {} -> {}", proxy_name, listen_addr, target_addr);

    let mut buf = vec![0u8; 65535];
    let session_timeout = Duration::from_secs(300);

    // 启动会话清理任务
    let udp_sessions_cleanup = udp_sessions.clone();
    let client_id_clone = client_id.clone();
    let proxy_name_clone = proxy_name.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let mut sessions = udp_sessions_cleanup.write().await;
            let key = (client_id_clone.clone(), proxy_id);
            if let Some(session_map) = sessions.get_mut(&key) {
                let now = tokio::time::Instant::now();
                session_map.retain(|addr, session| {
                    if now.duration_since(session.last_activity) > session_timeout {
                        debug!("[{}] UDP会话超时: {}", proxy_name_clone, addr);
                        false
                    } else {
                        true
                    }
                });
            }
        }
    });

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, src_addr)) => {
                let data = buf[..len].to_vec();
                let conn_provider_clone = conn_provider.clone();
                let client_id = client_id.clone();
                let target_addr = target_addr.clone();
                let proxy_name = proxy_name.clone();
                let udp_sessions = udp_sessions.clone();
                let socket = socket.clone();
                let traffic_manager = traffic_manager.clone();

                tokio::spawn(async move {
                    if let Err(e) = handle_udp_to_tunnel_unified(
                        socket,
                        src_addr,
                        data,
                        target_addr,
                        proxy_name,
                        client_id,
                        conn_provider_clone,
                        proxy_id,
                        udp_sessions,
                        traffic_manager,
                    ).await {
                        error!("❌ 处理UDP错误: {}", e);
                    }
                });
            }
            Err(e) => {
                error!("[{}] ❌ 接收UDP数据失败: {}", proxy_name, e);
            }
        }
    }
}

async fn handle_tcp_to_tunnel_unified(
    mut tcp_stream: TcpStream,
    addr: std::net::SocketAddr,
    target_addr: String,
    proxy_name: String,
    client_id: String,
    conn_provider: ConnectionProvider,
    proxy_id: i64,
    traffic_manager: Arc<TrafficManager>,
    speed_limiter: Arc<super::speed_limiter::SpeedLimiter>,
) -> Result<()> {
    // 获取统一连接
    let conn = match conn_provider.get_connection(&client_id).await {
        Some(c) => c,
        None => {
            error!("[{}] ❌ 客户端未连接", proxy_name);
            return Ok(());
        }
    };

    // 打开双向流
    let (mut tunnel_send, mut tunnel_recv) = conn.open_bi().await?;

    info!("[{}] 🔗 隧道流已打开: {}", proxy_name, addr);

    // 发送消息类型 + 协议类型 + 目标地址 (格式: 1字节消息类型'p' + 1字节协议类型 + 2字节长度 + 地址)
    tunnel_send.write_all(&[b'p']).await?; // 'p' 表示代理请求
    tunnel_send.write_all(&[b't']).await?; // 't' 表示TCP
    let target_bytes = target_addr.as_bytes();
    let len = target_bytes.len() as u16;

    tunnel_send.write_all(&len.to_be_bytes()).await?;
    tunnel_send.write_all(target_bytes).await?;
    tunnel_send.flush().await?;

    let (mut tcp_read, mut tcp_write) = tcp_stream.split();

    // 使用 AtomicI64 在两个方向上统计流量（无锁，性能更好）
    let sent_stats = Arc::new(std::sync::atomic::AtomicI64::new(0));
    let received_stats = Arc::new(std::sync::atomic::AtomicI64::new(0));

    let sent_stats_clone = sent_stats.clone();
    let received_stats_clone = received_stats.clone();

    // TCP -> Tunnel
    let proxy_name_t2t = proxy_name.clone();
    let speed_limiter_t2t = speed_limiter.clone();
    let tcp_to_tunnel = async move {
        let mut buf = vec![0u8; 32768];
        loop {
            let n = tcp_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            speed_limiter_t2t.consume(n).await;
            tunnel_send.write_all(&buf[..n]).await?;
            sent_stats_clone.fetch_add(n as i64, std::sync::atomic::Ordering::Relaxed);
        }
        // 关闭发送端，通知对端不再有数据
        let _ = tunnel_send.finish().await;
        Ok::<_, anyhow::Error>(())
    };

    // Tunnel -> TCP
    let proxy_name_t2c = proxy_name.clone();
    let speed_limiter_t2c = speed_limiter.clone();
    let tunnel_to_tcp = async move {
        let mut buf = vec![0u8; 32768];
        loop {
            match tunnel_recv.read(&mut buf).await? {
                Some(n) => {
                    if n == 0 {
                        break;
                    }
                    speed_limiter_t2c.consume(n).await;
                    tcp_write.write_all(&buf[..n]).await?;
                    received_stats_clone.fetch_add(n as i64, std::sync::atomic::Ordering::Relaxed);
                }
                None => break,
            }
        }
        let _ = tcp_write.shutdown().await;
        Ok::<_, anyhow::Error>(())
    };

    // 使用 join! 确保两个方向都完成，避免 select! 取消导致流量统计丢失
    let (res_t2t, res_t2c) = tokio::join!(tcp_to_tunnel, tunnel_to_tcp);
    if let Err(e) = res_t2t {
        debug!("[{}] TCP->Tunnel结束: {}", proxy_name_t2t, e);
    }
    if let Err(e) = res_t2c {
        debug!("[{}] Tunnel->TCP结束: {}", proxy_name_t2c, e);
    }

    info!("[{}] 🔚 连接已关闭: {}", proxy_name, addr);

    // 获取最终统计数据
    let bytes_sent = sent_stats.load(std::sync::atomic::Ordering::Relaxed);
    let bytes_received = received_stats.load(std::sync::atomic::Ordering::Relaxed);

    // 记录流量统计
    if bytes_sent > 0 || bytes_received > 0 {
        let client_id_num = client_id.parse::<i64>().unwrap_or(0);

        // 1. 记录 proxy/client/daily 维度的流量
        traffic_manager.record_traffic(
            proxy_id,
            client_id_num,
            None,
            bytes_sent,
            bytes_received,
        ).await;

        debug!("[{}] 流量统计(远程): 发送={}, 接收={}",
               proxy_name, bytes_sent, bytes_received);
    }

    Ok(())
}

async fn handle_udp_to_tunnel_unified(
    socket: Arc<UdpSocket>,
    src_addr: SocketAddr,
    data: Vec<u8>,
    target_addr: String,
    proxy_name: String,
    client_id: String,
    conn_provider: ConnectionProvider,
    proxy_id: i64,
    _udp_sessions: Arc<RwLock<HashMap<(String, i64), HashMap<SocketAddr, UdpSession>>>>,
    traffic_manager: Arc<TrafficManager>,
) -> Result<()> {
    // 获取统一连接
    let conn = match conn_provider.get_connection(&client_id).await {
        Some(c) => c,
        None => {
            error!("[{}] ❌ 客户端未连接", proxy_name);
            return Ok(());
        }
    };

    // 打开双向流
    let (mut tunnel_send, mut tunnel_recv) = conn.open_bi().await?;

    info!("[{}] 🔗 UDP隧道流已打开: {}", proxy_name, src_addr);

    // 发送消息类型 + 协议类型 + 目标地址 (格式: 1字节消息类型'p' + 1字节协议类型 + 2字节长度 + 地址)
    tunnel_send.write_all(&[b'p']).await?; // 'p' 表示代理请求
    tunnel_send.write_all(&[b'u']).await?; // 'u' 表示UDP
    let target_bytes = target_addr.as_bytes();
    let len = target_bytes.len() as u16;
    tunnel_send.write_all(&len.to_be_bytes()).await?;
    tunnel_send.write_all(target_bytes).await?;
    tunnel_send.write_all(&data).await?;
    tunnel_send.flush().await?;

    let bytes_sent = data.len() as i64;

    // 读取响应并转发回源
    let mut recv_buf = vec![0u8; 65535];
    let mut bytes_received = 0i64;

    loop {
        match tunnel_recv.read(&mut recv_buf).await? {
            Some(n) => {
                if n == 0 {
                    break;
                }
                bytes_received += n as i64;
                socket.send_to(&recv_buf[..n], src_addr).await?;
            }
            None => break,
        }
    }

    tunnel_send.finish().await?;

    // 统一记录流量
    if bytes_sent > 0 || bytes_received > 0 {
        let client_id_num = client_id.parse::<i64>().unwrap_or(0);

        // 1. 记录 proxy/client/daily 维度的流量
        traffic_manager.record_traffic(
            proxy_id,
            client_id_num,
            None,
            bytes_sent,
            bytes_received,
        ).await;
    }

    Ok(())
}
