//! AgentServerService gRPC 实现
//!
//! 处理 Agent Server 与 Controller 之间的双向流通信。

use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{error, info, warn};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use chrono::Utc;

use common::grpc::oxiproxy;
use common::grpc::oxiproxy::agent_server_message::Payload as AgentPayload;
use common::grpc::oxiproxy::controller_to_agent_message::Payload as ControllerPayload;
use common::grpc::AgentServerService;

use crate::local_auth_provider::LocalControllerAuthProvider;
use crate::node_manager::NodeManager;
use crate::entity::{Node, Proxy, node, proxy};
use crate::migration::get_connection;
use crate::certificate_manager::CertificateManager;

use common::protocol::auth::ClientAuthProvider;

pub struct AgentServerServiceImpl {
    pub node_manager: Arc<NodeManager>,
}

type ResponseStream = Pin<Box<dyn Stream<Item = Result<oxiproxy::ControllerToAgentMessage, Status>> + Send>>;

#[tonic::async_trait]
impl AgentServerService for AgentServerServiceImpl {
    type AgentServerChannelStream = ResponseStream;

    async fn agent_server_channel(
        &self,
        request: Request<Streaming<oxiproxy::AgentServerMessage>>,
    ) -> Result<Response<Self::AgentServerChannelStream>, Status> {
        // 在消费 request 之前提取客户端 IP
        let client_ip = crate::geo_ip::extract_client_ip_from_request(&request);

        let mut in_stream = request.into_inner();
        let (tx, rx) = mpsc::channel::<Result<oxiproxy::ControllerToAgentMessage, Status>>(256);

        let node_manager = self.node_manager.clone();

        tokio::spawn(async move {
            // 1. 读取首条消息，必须是认证请求
            let first_msg = match in_stream.next().await {
                Some(Ok(msg)) => msg,
                Some(Err(e)) => {
                    error!("读取认证消息失败: {}", e);
                    return;
                }
                None => {
                    error!("流在认证前关闭");
                    return;
                }
            };

            let register_req = match first_msg.payload {
                Some(AgentPayload::Register(req)) => req,
                _ => {
                    error!("首条消息必须是认证请求");
                    let _ = tx.send(Err(Status::invalid_argument("首条消息必须是认证请求"))).await;
                    return;
                }
            };

            // 2. 验证 token 并认证节点
            let db = get_connection().await;
            let node_model = match Node::find()
                .filter(node::Column::Secret.eq(&register_req.token))
                .one(db)
                .await
            {
                Ok(Some(n)) => n,
                Ok(None) => {
                    error!("无效的节点 token");
                    let _ = tx.send(Err(Status::unauthenticated("无效的节点 token"))).await;
                    return;
                }
                Err(e) => {
                    error!("查询节点失败: {}", e);
                    let _ = tx.send(Err(Status::internal(format!("数据库错误: {}", e)))).await;
                    return;
                }
            };

            let node_id = node_model.id;
            let node_name = node_model.name.clone();
            let authoritative_protocol = node_model.tunnel_protocol.clone();
            let node_speed_limit = node_model.speed_limit;
            let node_tunnel_port = node_model.tunnel_port;
            let current_tunnel_addr = node_model.tunnel_addr.clone();

            // 查询地理位置信息
            let geo_info = if let Some(ref ip) = client_ip {
                crate::geo_ip::query_geo_ip(ip).await.ok()
            } else {
                None
            };

            // 更新节点信息（tunnel_port 和 tunnel_protocol 均以 Controller DB 为权威来源）
            let mut active: crate::entity::node::ActiveModel = node_model.into();
            active.is_online = Set(true);
            active.updated_at = Set(Utc::now().naive_utc());
            active.version = Set(if register_req.version.is_empty() { None } else { Some(register_req.version.clone()) });

            // 更新公网IP和地理位置
            if let Some(geo) = geo_info {
                // 如果隧道地址为空，自动设置为公网IP
                if current_tunnel_addr.is_empty() {
                    active.tunnel_addr = Set(geo.ip.clone());
                }
                active.public_ip = Set(Some(geo.ip));
                active.region = Set(Some(geo.region));
            } else if let Some(ip) = client_ip {
                if current_tunnel_addr.is_empty() {
                    active.tunnel_addr = Set(ip.clone());
                }
                active.public_ip = Set(Some(ip));
            }

            if let Err(e) = active.update(db).await {
                error!("更新节点 #{} 失败: {}", node_id, e);
            }

            info!("节点 #{} ({}) 已通过 gRPC 连接认证", node_id, node_name);

            // 获取或生成节点证书
            let certificate = match CertificateManager::get_or_generate_node_certificate(node_id, &node_name).await {
                Ok(cert_data) => {
                    info!("✅ 节点 #{} 证书已准备（指纹: {}）", node_id, cert_data.fingerprint);
                    Some(oxiproxy::NodeCertificate {
                        cert_pem: cert_data.cert_pem,
                        key_pem: cert_data.key_pem,
                        fingerprint: cert_data.fingerprint,
                        expires_at: cert_data.expires_at,
                    })
                }
                Err(e) => {
                    error!("生成节点 #{} 证书失败: {}", node_id, e);
                    None
                }
            };

            // 发送认证响应（包含权威隧道协议、隧道端口和证书）
            let register_resp = oxiproxy::ControllerToAgentMessage {
                payload: Some(ControllerPayload::RegisterResponse(oxiproxy::NodeRegisterResponse {
                    node_id,
                    node_name: node_name.clone(),
                    tunnel_protocol: authoritative_protocol,
                    speed_limit: node_speed_limit,
                    tunnel_port: node_tunnel_port as u32,
                    certificate,
                })),
            };
            if tx.send(Ok(register_resp)).await.is_err() {
                return;
            }

            // 3. 将 stream sender 注册到 NodeManager
            node_manager.register_node_stream(node_id, tx.clone()).await;

            // 4. 消息处理循环
            let auth_provider = LocalControllerAuthProvider::new();

            while let Some(result) = in_stream.next().await {
                let msg = match result {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("节点 #{} 流错误: {}", node_id, e);
                        break;
                    }
                };

                let payload = match msg.payload {
                    Some(p) => p,
                    None => continue,
                };

                match payload {
                    AgentPayload::Heartbeat(hb) => {
                        let resp = oxiproxy::ControllerToAgentMessage {
                            payload: Some(ControllerPayload::HeartbeatResponse(oxiproxy::Heartbeat {
                                timestamp: hb.timestamp,
                            })),
                        };
                        let _ = tx.send(Ok(resp)).await;
                    }

                    AgentPayload::ValidateToken(req) => {
                        let result = auth_provider.validate_token(&req.token).await;
                        let resp = match result {
                            Ok(r) => oxiproxy::ValidateTokenResponse {
                                request_id: req.request_id,
                                client_id: r.client_id,
                                client_name: r.client_name,
                                allowed: r.allowed,
                                reject_reason: r.reject_reason,
                            },
                            Err(e) => oxiproxy::ValidateTokenResponse {
                                request_id: req.request_id,
                                client_id: 0,
                                client_name: String::new(),
                                allowed: false,
                                reject_reason: Some(e.to_string()),
                            },
                        };
                        let msg = oxiproxy::ControllerToAgentMessage {
                            payload: Some(ControllerPayload::ValidateTokenResponse(resp)),
                        };
                        let _ = tx.send(Ok(msg)).await;
                    }

                    AgentPayload::ClientOnline(req) => {
                        let success = auth_provider
                            .set_client_online(req.client_id, req.online)
                            .await
                            .is_ok();
                        let resp = oxiproxy::ControllerToAgentMessage {
                            payload: Some(ControllerPayload::ClientOnlineResponse(
                                oxiproxy::ClientOnlineResponse {
                                    request_id: req.request_id,
                                    success,
                                },
                            )),
                        };
                        let _ = tx.send(Ok(resp)).await;
                    }

                    AgentPayload::CheckTrafficLimit(req) => {
                        let result = auth_provider.check_traffic_limit(req.client_id).await;
                        let resp = match result {
                            Ok(r) => oxiproxy::TrafficLimitResponse {
                                request_id: req.request_id,
                                exceeded: r.exceeded,
                                reason: r.reason,
                            },
                            Err(_) => oxiproxy::TrafficLimitResponse {
                                request_id: req.request_id,
                                exceeded: false,
                                reason: None,
                            },
                        };
                        let msg = oxiproxy::ControllerToAgentMessage {
                            payload: Some(ControllerPayload::TrafficLimitResponse(resp)),
                        };
                        let _ = tx.send(Ok(msg)).await;
                    }

                    AgentPayload::GetClientProxies(req) => {
                        let proxies = get_client_proxies_filtered(
                            req.client_id,
                            req.node_id,
                        ).await;
                        let resp = oxiproxy::GetClientProxiesResponse {
                            request_id: req.request_id,
                            proxies,
                        };
                        let msg = oxiproxy::ControllerToAgentMessage {
                            payload: Some(ControllerPayload::GetClientProxiesResponse(resp)),
                        };
                        let _ = tx.send(Ok(msg)).await;
                    }

                    AgentPayload::TrafficReport(req) => {
                        // 处理流量上报
                        let traffic_manager = crate::traffic::TrafficManager::new();
                        for record in req.records {
                            let cid = record.client_id.parse::<i64>().unwrap_or(0);
                            traffic_manager
                                .record_traffic(
                                    record.proxy_id,
                                    cid,
                                    record.user_id,
                                    record.bytes_sent,
                                    record.bytes_received,
                                )
                                .await;
                        }
                        let resp = oxiproxy::ControllerToAgentMessage {
                            payload: Some(ControllerPayload::TrafficReportResponse(
                                oxiproxy::TrafficReportResponse { accepted: true },
                            )),
                        };
                        let _ = tx.send(Ok(resp)).await;
                    }

                    AgentPayload::Response(resp) => {
                        // Agent Server 对 Controller 指令的响应
                        node_manager.complete_pending_request(node_id, &resp).await;
                    }

                    _ => {
                        warn!("节点 #{} 收到未知消息类型", node_id);
                    }
                }
            }

            // 5. 清理：标记节点离线
            info!("节点 #{} ({}) gRPC 连接断开", node_id, node_name);
            node_manager.unregister_node_stream(node_id).await;

            let db = get_connection().await;
            if let Ok(Some(n)) = Node::find_by_id(node_id).one(db).await {
                let mut active: crate::entity::node::ActiveModel = n.into();
                active.is_online = Set(false);
                active.updated_at = Set(Utc::now().naive_utc());
                let _ = active.update(db).await;
            }
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream) as Self::AgentServerChannelStream))
    }
}

/// 获取客户端代理配置（支持 node_id 过滤）
async fn get_client_proxies_filtered(client_id: i64, filter_node_id: i64) -> Vec<oxiproxy::ProxyConfig> {
    let db = get_connection().await;
    let client_id_str = client_id.to_string();

    let proxies = match Proxy::find()
        .filter(proxy::Column::ClientId.eq(&client_id_str))
        .filter(proxy::Column::Enabled.eq(true))
        .all(db)
        .await
    {
        Ok(p) => p,
        Err(_) => return vec![],
    };

    // 过滤出属于指定节点的代理
    proxies
        .into_iter()
        .filter(|p| p.node_id == Some(filter_node_id))
        .map(|p| oxiproxy::ProxyConfig {
            proxy_id: p.id,
            client_id: p.client_id,
            name: p.name,
            proxy_type: p.proxy_type,
            local_ip: p.local_ip,
            local_port: p.local_port as u32,
            remote_port: p.remote_port as u32,
            enabled: p.enabled,
        })
        .collect()
}
