//! 客户端认证 trait 和相关类型
//!
//! 定义了 frps 验证客户端身份的接口，
//! 可由本地 DB 实现或通过 HTTP 调用 Controller 实现。

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::control::ProxyConfig;

/// Token 验证请求
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateTokenRequest {
    pub token: String,
}

/// Token 验证响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateTokenResponse {
    pub client_id: i64,
    pub client_name: String,
    pub allowed: bool,
    pub reject_reason: Option<String>,
}

/// 客户端上下线通知
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientOnlineRequest {
    pub online: bool,
}

/// 流量限制检查响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrafficLimitResponse {
    pub exceeded: bool,
    pub reason: Option<String>,
}

/// 客户端认证提供者接口
///
/// 由 Controller 中的本地 DB 实现（Phase 0），
/// 或通过 HTTP 调用 Controller 实现（Phase 1）。
#[async_trait]
pub trait ClientAuthProvider: Send + Sync {
    /// 验证客户端 token
    ///
    /// 返回客户端信息和是否允许连接
    async fn validate_token(&self, token: &str) -> Result<ValidateTokenResponse>;

    /// 通知客户端上下线状态
    async fn set_client_online(&self, client_id: i64, online: bool) -> Result<()>;

    /// 检查客户端是否超出流量限制
    async fn check_traffic_limit(&self, client_id: i64) -> Result<TrafficLimitResponse>;

    /// Check a Node-owned proxy against its owner and node traffic quotas.
    async fn check_direct_proxy_limit(&self, _proxy_id: i64) -> Result<TrafficLimitResponse> {
        anyhow::bail!("不支持公网代理流量检查")
    }

    /// 获取客户端的所有代理配置
    async fn get_client_proxies(&self, client_id: i64) -> Result<Vec<ProxyConfig>>;
}
