//! gRPC 认证提供者
//!
//! 通过 gRPC 双向流实现 ClientAuthProvider trait，
//! 替代原有的 HTTP 远程认证。

use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use async_trait::async_trait;
use tracing::debug;

use common::grpc::oxiproxy;
use common::grpc::oxiproxy::agent_server_message::Payload as AgentPayload;
use common::grpc::pending_requests::PendingRequests;
use common::protocol::auth::{
    ClientAuthProvider, TrafficLimitResponse, ValidateTokenResponse,
};
use common::protocol::control::ProxyConfig;

use super::grpc_client::{AgentGrpcClient, ControllerResponse, SharedGrpcSender, SharedPendingRequests};

/// gRPC 认证提供者
pub struct GrpcAuthProvider {
    sender: SharedGrpcSender,
    pending: SharedPendingRequests,
    node_id: i64,
}

impl GrpcAuthProvider {
    pub fn new(grpc_client: &Arc<AgentGrpcClient>, node_id: i64) -> Self {
        Self {
            sender: grpc_client.shared_sender().clone(),
            pending: grpc_client.shared_pending().clone(),
            node_id,
        }
    }
}

#[async_trait]
impl ClientAuthProvider for GrpcAuthProvider {
    async fn validate_token(&self, token: &str) -> Result<ValidateTokenResponse> {
        let (request_id, rx) = self.pending.register().await;
        debug!("gRPC 验证 token (request_id={})", request_id);

        let msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::ValidateToken(oxiproxy::ValidateTokenRequest {
                request_id: request_id.clone(),
                token: token.to_string(),
            })),
        };

        self.sender.send(msg).await
            .map_err(|_| anyhow::anyhow!("发送验证请求失败"))?;

        let resp = PendingRequests::wait(rx, Duration::from_secs(10)).await?;

        match resp {
            ControllerResponse::ValidateToken(r) => {
                Ok(ValidateTokenResponse {
                    client_id: r.client_id,
                    client_name: r.client_name,
                    allowed: r.allowed,
                    reject_reason: r.reject_reason,
                })
            }
            _ => Err(anyhow::anyhow!("收到意外的响应类型")),
        }
    }

    async fn set_client_online(&self, client_id: i64, online: bool) -> Result<()> {
        let (request_id, rx) = self.pending.register().await;
        debug!("gRPC 上报客户端 #{} 状态: online={}", client_id, online);

        let msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::ClientOnline(oxiproxy::ClientOnlineRequest {
                request_id: request_id.clone(),
                client_id,
                online,
            })),
        };

        self.sender.send(msg).await
            .map_err(|_| anyhow::anyhow!("发送客户端状态请求失败"))?;

        // 等待响应（但不严格要求成功）
        let _ = PendingRequests::wait(rx, Duration::from_secs(5)).await;
        Ok(())
    }

    async fn check_traffic_limit(&self, client_id: i64) -> Result<TrafficLimitResponse> {
        let (request_id, rx) = self.pending.register().await;
        debug!("gRPC 检查客户端 #{} 流量限制", client_id);

        let msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::CheckTrafficLimit(oxiproxy::CheckTrafficLimitRequest {
                request_id: request_id.clone(),
                client_id,
            })),
        };

        self.sender.send(msg).await
            .map_err(|_| anyhow::anyhow!("发送流量检查请求失败"))?;

        let resp = PendingRequests::wait(rx, Duration::from_secs(10)).await?;

        match resp {
            ControllerResponse::TrafficLimit(r) => {
                Ok(TrafficLimitResponse {
                    exceeded: r.exceeded,
                    reason: r.reason,
                })
            }
            _ => Err(anyhow::anyhow!("收到意外的响应类型")),
        }
    }

    async fn get_client_proxies(&self, client_id: i64) -> Result<Vec<ProxyConfig>> {
        let (request_id, rx) = self.pending.register().await;
        debug!("gRPC 获取客户端 #{} 代理配置 (node_id={})", client_id, self.node_id);

        let msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::GetClientProxies(oxiproxy::GetClientProxiesRequest {
                request_id: request_id.clone(),
                client_id,
                node_id: self.node_id,
            })),
        };

        self.sender.send(msg).await
            .map_err(|_| anyhow::anyhow!("发送获取代理配置请求失败"))?;

        let resp = PendingRequests::wait(rx, Duration::from_secs(10)).await?;

        match resp {
            ControllerResponse::GetClientProxies(r) => {
                Ok(r.proxies.into_iter().map(|p| ProxyConfig {
                    proxy_id: p.proxy_id,
                    client_id: p.client_id,
                    name: p.name,
                    proxy_type: p.proxy_type,
                    domain: p.domain,
                    local_ip: p.local_ip,
                    local_port: p.local_port as u16,
                    remote_port: p.remote_port as u16,
                    enabled: p.enabled,
                }).collect())
            }
            _ => Err(anyhow::anyhow!("收到意外的响应类型")),
        }
    }
}
