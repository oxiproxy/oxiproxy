//! 本地代理控制实现
//!
//! 直接调用 ProxyListenerManager 和 ConnectionProvider，
//! 实现 ProxyControl trait，支持通过 gRPC 命令启停代理。

use std::sync::Arc;
use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::RwLock;
use std::collections::HashMap;
use tracing::info;

use common::protocol::auth::ClientAuthProvider;
use common::protocol::control::{
    ConnectedClient, LogEntry, ProxyControl, ServerStatus,
};
use common::TunnelConnection;

use crate::server::proxy_server::{ConnectionProvider, ProxyListenerManager, ProxyServer};
use crate::server::client_logs;

/// 本地代理控制实现
///
/// 直接调用 ProxyServer 内部组件，无需网络通信。
pub struct LocalProxyControl {
    listener_manager: Arc<ProxyListenerManager>,
    quic_connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
    tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
    auth_provider: Arc<dyn ClientAuthProvider>,
    proxy_server: Arc<ProxyServer>,
}

impl LocalProxyControl {
    pub fn new(
        listener_manager: Arc<ProxyListenerManager>,
        quic_connections: Arc<RwLock<HashMap<String, Arc<quinn::Connection>>>>,
        tunnel_connections: Arc<RwLock<HashMap<String, Arc<Box<dyn TunnelConnection>>>>>,
        auth_provider: Arc<dyn ClientAuthProvider>,
        proxy_server: Arc<ProxyServer>,
    ) -> Self {
        Self {
            listener_manager,
            quic_connections,
            tunnel_connections,
            auth_provider,
            proxy_server,
        }
    }

    fn conn_provider(&self) -> ConnectionProvider {
        ConnectionProvider::new(
            self.quic_connections.clone(),
            self.tunnel_connections.clone(),
        )
    }
}

#[async_trait]
impl ProxyControl for LocalProxyControl {
    async fn start_proxy(&self, client_id: &str, proxy_id: i64) -> Result<()> {
        // 先停止旧的监听器（如果存在），确保配置更新时能正确重启
        self.listener_manager.stop_single_proxy(client_id, proxy_id).await;

        // 通过 auth_provider 获取该客户端的代理配置
        let client_id_num: i64 = client_id.parse()
            .map_err(|_| anyhow::anyhow!("无效的 client_id: {}", client_id))?;
        let all_proxies = self.auth_provider.get_client_proxies(client_id_num).await?;

        // 过滤出目标代理
        let target_proxies: Vec<_> = all_proxies.into_iter()
            .filter(|p| p.proxy_id == proxy_id && p.enabled)
            .collect();

        if target_proxies.is_empty() {
            return Err(anyhow::anyhow!(
                "未找到代理配置: client_id={}, proxy_id={}", client_id, proxy_id
            ));
        }

        info!("启动代理: client_id={}, proxy_id={}", client_id, proxy_id);

        // 使用 ProxyListenerManager 启动代理监听器
        self.listener_manager.start_client_proxies_from_configs(
            client_id.to_string(),
            target_proxies,
            self.conn_provider(),
        ).await
    }

    async fn stop_proxy(&self, client_id: &str, proxy_id: i64) -> Result<()> {
        self.listener_manager
            .stop_single_proxy(client_id, proxy_id)
            .await;
        Ok(())
    }

    async fn get_connected_clients(&self) -> Result<Vec<ConnectedClient>> {
        let mut clients = Vec::new();

        // QUIC connections
        {
            let conns = self.quic_connections.read().await;
            for (client_id, conn) in conns.iter() {
                clients.push(ConnectedClient {
                    client_id: client_id.clone(),
                    remote_address: conn.remote_address().to_string(),
                    protocol: "quic".to_string(),
                });
            }
        }

        // KCP/Tunnel connections
        {
            let conns = self.tunnel_connections.read().await;
            for (client_id, conn) in conns.iter() {
                clients.push(ConnectedClient {
                    client_id: client_id.clone(),
                    remote_address: conn.remote_address().to_string(),
                    protocol: "kcp".to_string(),
                });
            }
        }

        Ok(clients)
    }

    async fn fetch_client_logs(&self, client_id: &str, count: u16) -> Result<Vec<LogEntry>> {
        // 目前只支持 QUIC 连接获取日志
        let conn = {
            let conns = self.quic_connections.read().await;
            conns.get(client_id).cloned()
        };

        let conn = match conn {
            Some(c) => c,
            None => return Err(anyhow::anyhow!("客户端未连接或不在线")),
        };

        let logs = client_logs::fetch_client_logs(conn, count).await?;
        Ok(logs
            .into_iter()
            .map(|l| LogEntry {
                timestamp: l.timestamp.to_rfc3339(),
                level: l.level,
                message: l.message,
            })
            .collect())
    }

    async fn get_server_status(&self) -> Result<ServerStatus> {
        let clients = self.get_connected_clients().await?;
        let active_proxy_count = clients.len(); // 简化：用连接数近似
        Ok(ServerStatus {
            connected_clients: clients,
            active_proxy_count,
        })
    }

    async fn update_certificate(&self, cert_pem: String, key_pem: String) -> Result<()> {
        self.proxy_server.update_certificate(cert_pem, key_pem).await
    }
}
