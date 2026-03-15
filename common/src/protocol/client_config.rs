//! 客户端连接配置协议类型
//!
//! 定义了 Agent Client 从 Controller 获取连接配置的请求/响应结构体。

use serde::{Deserialize, Serialize};
use crate::config::KcpConfig;
use crate::tunnel::TunnelProtocol;

/// 客户端连接配置请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConnectConfigRequest {
    pub token: String,
}

/// Controller 返回给 Agent Client 的连接配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientConnectConfig {
    /// Agent Server 地址
    pub server_addr: String,
    /// Agent Server 端口
    pub server_port: u16,
    /// 隧道协议类型
    pub protocol: TunnelProtocol,
    /// KCP 配置（可选）
    pub kcp: Option<KcpConfig>,
    /// 客户端 ID
    pub client_id: i64,
    /// 客户端名称
    pub client_name: String,
}

/// 客户端轮询代理列表请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientPollRequest {
    pub token: String,
}

/// 客户端轮询代理列表响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientPollResponse {
    pub client_id: i64,
    pub client_name: String,
    /// 按 Server 分组的代理列表
    pub server_groups: Vec<ServerProxyGroup>,
}

/// 一个 Server 及其上面需要运行的代理
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerProxyGroup {
    /// 节点 ID
    pub node_id: i64,
    /// Server 隧道地址
    pub server_addr: String,
    /// Server 隧道端口
    pub server_port: u16,
    /// 隧道协议
    pub protocol: TunnelProtocol,
    /// KCP 配置（可选）
    pub kcp: Option<KcpConfig>,
    /// 该 Server 上的代理列表
    pub proxies: Vec<ProxyInfo>,
    /// 证书指纹列表（用于 QUIC 证书验证）
    #[serde(default)]
    pub cert_fingerprints: Vec<String>,
}

/// 轮询响应中的代理信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyInfo {
    pub proxy_id: i64,
    pub name: String,
    pub proxy_type: String,
    pub local_ip: String,
    pub local_port: i32,
    pub remote_port: i32,
    pub enabled: bool,
}
