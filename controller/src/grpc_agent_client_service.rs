//! AgentClientService gRPC 实现
//!
//! 处理 Agent Client 与 Controller 之间的双向流通信。

use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_stream::{wrappers::ReceiverStream, Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tracing::{debug, error, info, warn};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use chrono::Utc;

use common::grpc::oxiproxy;
use common::grpc::oxiproxy::agent_client_message::Payload as ClientPayload;
use common::grpc::oxiproxy::controller_to_client_message::Payload as ControllerPayload;
use common::grpc::AgentClientService;

use crate::client_stream_manager::ClientStreamManager;
use crate::entity::{Client, client};
use crate::migration::get_connection;

pub struct AgentClientServiceImpl {
    pub client_stream_manager: Arc<ClientStreamManager>,
}

type ResponseStream = Pin<Box<dyn Stream<Item = Result<oxiproxy::ControllerToClientMessage, Status>> + Send>>;

#[tonic::async_trait]
impl AgentClientService for AgentClientServiceImpl {
    type AgentClientChannelStream = ResponseStream;

    async fn agent_client_channel(
        &self,
        request: Request<Streaming<oxiproxy::AgentClientMessage>>,
    ) -> Result<Response<Self::AgentClientChannelStream>, Status> {
        let client_ip = crate::geo_ip::extract_client_ip_from_request(&request);
        let mut in_stream = request.into_inner();
        let (tx, rx) = mpsc::channel::<Result<oxiproxy::ControllerToClientMessage, Status>>(256);

        let client_stream_manager = self.client_stream_manager.clone();

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

            let auth_req = match first_msg.payload {
                Some(ClientPayload::Auth(req)) => req,
                _ => {
                    let _ = tx.send(Err(Status::invalid_argument("首条消息必须是认证请求"))).await;
                    return;
                }
            };
            let client_version = if auth_req.version.is_empty() { None } else { Some(auth_req.version.clone()) };

            // 2. 验证 token
            let db = get_connection().await;
            let client_model = match Client::find()
                .filter(client::Column::Token.eq(&auth_req.token))
                .one(db)
                .await
            {
                Ok(Some(c)) => c,
                Ok(None) => {
                    let resp = oxiproxy::ControllerToClientMessage {
                        payload: Some(ControllerPayload::AuthResponse(oxiproxy::ClientAuthResponse {
                            success: false,
                            error_message: Some("无效的 token".to_string()),
                            client_id: 0,
                            client_name: String::new(),
                        })),
                    };
                    let _ = tx.send(Ok(resp)).await;
                    return;
                }
                Err(e) => {
                    let resp = oxiproxy::ControllerToClientMessage {
                        payload: Some(ControllerPayload::AuthResponse(oxiproxy::ClientAuthResponse {
                            success: false,
                            error_message: Some(format!("数据库错误: {}", e)),
                            client_id: 0,
                            client_name: String::new(),
                        })),
                    };
                    let _ = tx.send(Ok(resp)).await;
                    return;
                }
            };

            // 检查流量限制
            if client_model.is_traffic_exceeded {
                let resp = oxiproxy::ControllerToClientMessage {
                    payload: Some(ControllerPayload::Error(oxiproxy::ErrorNotification {
                        code: "traffic_exceeded".to_string(),
                        message: format!("客户端 '{}' 流量已超限", client_model.name),
                    })),
                };
                let _ = tx.send(Ok(resp)).await;
                return;
            }

            let client_id = client_model.id;
            let client_name = client_model.name.clone();

            // 发送认证成功响应
            let auth_resp = oxiproxy::ControllerToClientMessage {
                payload: Some(ControllerPayload::AuthResponse(oxiproxy::ClientAuthResponse {
                    success: true,
                    error_message: None,
                    client_id,
                    client_name: client_name.clone(),
                })),
            };
            if tx.send(Ok(auth_resp)).await.is_err() {
                return;
            }

            info!("Agent Client #{} ({}) 已通过 gRPC 认证", client_id, client_name);

            // 更新客户端为在线状态
            let mut client_active: client::ActiveModel = client_model.into();
            client_active.is_online = Set(true);
            client_active.version = Set(client_version);
            if let Some(ref ip) = client_ip {
                client_active.public_ip = Set(Some(ip.clone()));
            }
            client_active.updated_at = Set(Utc::now().naive_utc());
            if let Err(e) = client_active.update(db).await {
                error!("更新客户端 #{} 在线状态失败: {}", client_id, e);
            }

            // 3. 先注册到 ClientStreamManager（避免注册前丢失代理更新通知）
            client_stream_manager.register(client_id, tx.clone()).await;

            // 4. 再推送当前代理列表（注册后推送，确保期间的变更不会丢失）
            match client_stream_manager.build_proxy_list_update(client_id).await {
                Ok(update) => {
                    let msg = oxiproxy::ControllerToClientMessage {
                        payload: Some(ControllerPayload::ProxyUpdate(update)),
                    };
                    if tx.send(Ok(msg)).await.is_err() {
                        return;
                    }
                }
                Err(e) => {
                    error!("构建初始代理列表失败: {}", e);
                }
            }

            // 5. 消息处理循环（主要处理心跳）
            while let Some(result) = in_stream.next().await {
                let msg = match result {
                    Ok(m) => m,
                    Err(e) => {
                        warn!("Client #{} 流错误: {}", client_id, e);
                        break;
                    }
                };

                let payload = match msg.payload {
                    Some(p) => p,
                    None => continue,
                };

                match payload {
                    ClientPayload::Heartbeat(hb) => {
                        let resp = oxiproxy::ControllerToClientMessage {
                            payload: Some(ControllerPayload::HeartbeatResponse(oxiproxy::Heartbeat {
                                timestamp: hb.timestamp,
                            })),
                        };
                        let _ = tx.send(Ok(resp)).await;
                    }
                    ClientPayload::Response(resp) => {
                        client_stream_manager.complete_pending_request(client_id, &resp).await;
                    }
                    _ => {
                        debug!("Client #{} 收到未知消息类型", client_id);
                    }
                }
            }

            // 6. 清理
            info!("Agent Client #{} ({}) gRPC 连接断开", client_id, client_name);
            client_stream_manager.unregister(client_id).await;

            // 更新客户端为离线状态
            let db = get_connection().await;
            if let Ok(Some(c)) = Client::find_by_id(client_id).one(db).await {
                let mut client_active: client::ActiveModel = c.into();
                client_active.is_online = Set(false);
                client_active.updated_at = Set(Utc::now().naive_utc());
                let _ = client_active.update(db).await;
            }
        });

        let output_stream = ReceiverStream::new(rx);
        Ok(Response::new(Box::pin(output_stream) as Self::AgentClientChannelStream))
    }
}
