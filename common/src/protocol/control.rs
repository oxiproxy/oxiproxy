//! 代理控制 trait 和相关类型
//!
//! 定义了 Controller 控制 frps 代理监听器的接口。

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// 代理配置信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    pub proxy_id: i64,
    pub client_id: String,
    pub name: String,
    pub proxy_type: String,
    pub local_ip: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub enabled: bool,
}

/// 启动代理请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartProxyRequest {
    pub client_id: String,
    pub proxy_id: i64,
}

/// 停止代理请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StopProxyRequest {
    pub client_id: String,
    pub proxy_id: i64,
}

/// 连接的客户端信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedClient {
    pub client_id: String,
    pub remote_address: String,
    pub protocol: String,
}

/// frps 状态信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub connected_clients: Vec<ConnectedClient>,
    pub active_proxy_count: usize,
}

/// 日志条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

/// 代理控制接口
///
/// 由 frps 实现（本地直接调用），或由 Controller 通过 HTTP 远程调用。
/// 用于管理代理监听器的启停和状态查询。
#[async_trait]
pub trait ProxyControl: Send + Sync {
    /// 启动指定客户端的指定代理监听器
    async fn start_proxy(&self, client_id: &str, proxy_id: i64) -> Result<()>;

    /// 停止指定客户端的指定代理监听器
    async fn stop_proxy(&self, client_id: &str, proxy_id: i64) -> Result<()>;

    /// 获取当前连接的客户端列表
    async fn get_connected_clients(&self) -> Result<Vec<ConnectedClient>>;

    /// 获取客户端日志
    async fn fetch_client_logs(&self, client_id: &str, count: u16) -> Result<Vec<LogEntry>>;

    /// 获取服务器状态
    async fn get_server_status(&self) -> Result<ServerStatus>;

    /// 更新证书（热更新）
    async fn update_certificate(&self, cert_pem: String, key_pem: String) -> Result<()>;
}
