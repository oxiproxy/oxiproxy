//! 隧道生命周期管理器
//!
//! 管理隧道监听器的启动、停止和协议切换。
//! 通过 CancellationToken 实现可取消的监听循环。

use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, error, warn};

use crate::server::proxy_server::ProxyServer;
use common::KcpConfig;

pub struct TunnelManager {
    proxy_server: Arc<ProxyServer>,
    bind_port: RwLock<u16>,
    current_protocol: RwLock<String>,
    cancel_token: RwLock<Option<CancellationToken>>,
    listener_handle: RwLock<Option<JoinHandle<()>>>,
}

impl TunnelManager {
    pub fn new(proxy_server: Arc<ProxyServer>, bind_port: u16) -> Self {
        Self {
            proxy_server,
            bind_port: RwLock::new(bind_port),
            current_protocol: RwLock::new(String::new()),
            cancel_token: RwLock::new(None),
            listener_handle: RwLock::new(None),
        }
    }

    /// 启动隧道监听器
    pub async fn start(&self, protocol: &str, kcp_config: Option<KcpConfig>) -> anyhow::Result<()> {
        self.stop().await;

        let bind_addr = format!("0.0.0.0:{}", *self.bind_port.read().await);
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let proxy_server = self.proxy_server.clone();
        let proto = protocol.to_string();

        let handle = tokio::spawn(async move {
            tokio::select! {
                result = async {
                    match proto.as_str() {
                        "kcp" => {
                            info!("启动 KCP 隧道服务: {}", bind_addr);
                            proxy_server.run_kcp(bind_addr, kcp_config).await
                        }
                        "tcp" => {
                            info!("启动 TCP 隧道服务: {}", bind_addr);
                            proxy_server.run_tcp(bind_addr).await
                        }
                        _ => {
                            info!("启动 QUIC 隧道服务: {}", bind_addr);
                            proxy_server.run(bind_addr).await
                        }
                    }
                } => {
                    if let Err(e) = result {
                        error!("隧道服务错误: {}", e);
                    }
                }
                _ = cancel_clone.cancelled() => {
                    info!("隧道服务已停止");
                }
            }
        });

        *self.current_protocol.write().await = protocol.to_string();
        *self.cancel_token.write().await = Some(cancel);
        *self.listener_handle.write().await = Some(handle);

        Ok(())
    }

    /// 停止当前隧道监听器
    pub async fn stop(&self) {
        if let Some(cancel) = self.cancel_token.write().await.take() {
            info!("正在停止当前隧道监听器...");
            cancel.cancel();
        }
        if let Some(handle) = self.listener_handle.write().await.take() {
            // 等待任务结束，超时 5 秒
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(_) => {}
                Err(_) => {
                    warn!("隧道监听器停止超时，强制终止");
                }
            }
        }
    }

    /// 更新隧道端口（下次 start/switch_protocol 时生效）
    pub fn update_port(&self, new_port: u16) {
        // 使用 try_write 避免异步，端口更新总是在 switch_protocol 之前调用
        if let Ok(mut port) = self.bind_port.try_write() {
            if *port != new_port {
                info!("隧道端口变更: {} -> {}", *port, new_port);
                *port = new_port;
            }
        }
    }

    /// 切换协议
    pub async fn switch_protocol(&self, new_protocol: &str) -> anyhow::Result<()> {
        let current = self.current_protocol.read().await.clone();
        if current == new_protocol {
            info!("协议未变更 ({}), 无需切换", new_protocol);
            return Ok(());
        }

        info!("切换隧道协议: {} -> {}", current, new_protocol);

        // 停止后短暂等待端口释放
        self.stop().await;
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        self.start(new_protocol, None).await
    }
}
