//! Agent Server gRPC Client
//!
//! 连接 Controller 的 gRPC 双向流，处理认证、消息分发、命令执行。

use std::sync::Arc;
use std::time::Duration;
use anyhow::{anyhow, Result};
use tokio::sync::{mpsc, RwLock};
use tokio_stream::StreamExt;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::{error, info, warn};

use common::grpc::oxiproxy;
use common::grpc::oxiproxy::agent_server_message::Payload as AgentPayload;
use common::grpc::oxiproxy::controller_to_agent_message::Payload as ControllerPayload;
use common::grpc::oxiproxy::agent_server_response::Result as AgentResult;
use common::grpc::AgentServerServiceClient;
use common::grpc::pending_requests::PendingRequests;
use common::protocol::control::{ProxyControl, LogEntry};

/// gRPC 流发送器类型
pub type GrpcSender = mpsc::Sender<oxiproxy::AgentServerMessage>;

/// 可热替换的 gRPC 发送器（重连后更新内部 sender）
#[derive(Clone)]
pub struct SharedGrpcSender {
    inner: Arc<RwLock<GrpcSender>>,
}

impl SharedGrpcSender {
    pub fn new(sender: GrpcSender) -> Self {
        Self {
            inner: Arc::new(RwLock::new(sender)),
        }
    }

    /// 发送消息（使用当前 sender）
    pub async fn send(&self, msg: oxiproxy::AgentServerMessage) -> Result<(), mpsc::error::SendError<oxiproxy::AgentServerMessage>> {
        let sender = self.inner.read().await;
        sender.send(msg).await
    }

    /// 重连后替换内部 sender
    pub async fn replace(&self, new_sender: GrpcSender) {
        let mut sender = self.inner.write().await;
        *sender = new_sender;
    }
}

/// 可热替换的 PendingRequests（重连后更新）
#[derive(Clone)]
pub struct SharedPendingRequests {
    inner: Arc<RwLock<PendingRequests<ControllerResponse>>>,
}

impl SharedPendingRequests {
    pub fn new(pending: PendingRequests<ControllerResponse>) -> Self {
        Self {
            inner: Arc::new(RwLock::new(pending)),
        }
    }

    /// 注册一个等待响应的请求
    pub async fn register(&self) -> (String, tokio::sync::oneshot::Receiver<ControllerResponse>) {
        let pending = self.inner.read().await;
        pending.register().await
    }

    /// 重连后替换内部 pending
    pub async fn replace(&self, new_pending: PendingRequests<ControllerResponse>) {
        let mut pending = self.inner.write().await;
        *pending = new_pending;
    }

    /// 获取内部 PendingRequests 的克隆（供 message_loop 使用）
    pub async fn get_inner(&self) -> PendingRequests<ControllerResponse> {
        self.inner.read().await.clone()
    }
}

/// Agent Server gRPC 客户端
pub struct AgentGrpcClient {
    /// 可热替换的发送器
    shared_sender: SharedGrpcSender,
    /// 可热替换的 pending requests
    shared_pending: SharedPendingRequests,
    /// 节点 ID（连接认证后获得）
    node_id: RwLock<i64>,
}

/// 节点证书数据
#[derive(Debug, Clone)]
pub struct NodeCertificate {
    pub cert_pem: String,
    pub key_pem: String,
    pub fingerprint: String,
}

/// Controller 响应的包装类型
#[derive(Clone)]
pub enum ControllerResponse {
    ValidateToken(oxiproxy::ValidateTokenResponse),
    ClientOnline(oxiproxy::ClientOnlineResponse),
    TrafficLimit(oxiproxy::TrafficLimitResponse),
    GetClientProxies(oxiproxy::GetClientProxiesResponse),
    TrafficReport(oxiproxy::TrafficReportResponse),
}

impl AgentGrpcClient {
    /// 连接 Controller 并认证节点
    ///
    /// 返回 (gRPC 客户端, 命令接收器, Controller 下发的权威隧道协议, 速度限制, 隧道端口, 节点证书)
    pub async fn connect_and_authenticate(
        controller_url: &str,
        token: &str,
        tunnel_protocol: &str,
        tls_ca_cert: Option<&[u8]>,
    ) -> Result<(Arc<Self>, mpsc::Receiver<ControllerCommand>, String, Option<i64>, u16, Option<NodeCertificate>)> {
        let mut endpoint = Channel::from_shared(controller_url.to_string())?
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .tcp_keepalive(Some(Duration::from_secs(60)))
            .http2_keep_alive_interval(Duration::from_secs(30))
            .keep_alive_timeout(Duration::from_secs(10));

        if controller_url.starts_with("https://") {
            // 从 URL 中提取域名用于 SNI
            let domain = controller_url
                .trim_start_matches("https://")
                .split(':')
                .next()
                .ok_or_else(|| anyhow!("无法从 URL 提取域名"))?;

            let mut tls_config = ClientTlsConfig::new()
                .domain_name(domain)
                .with_webpki_roots();

            if let Some(ca_pem) = tls_ca_cert {
                info!("使用自定义 CA 证书进行 TLS 验证");
                tls_config = tls_config.ca_certificate(
                    tonic::transport::Certificate::from_pem(ca_pem)
                );
            }

            endpoint = endpoint.tls_config(tls_config)
                .map_err(|e| anyhow!("TLS 配置失败: {}", e))?;
        }

        let channel = endpoint.connect()
            .await
            .map_err(|e| anyhow!("连接 Controller gRPC 失败: {}", e))?;

        let mut client = AgentServerServiceClient::new(channel);

        // 创建双向流
        let (tx, rx) = mpsc::channel::<oxiproxy::AgentServerMessage>(256);
        let (cmd_tx, cmd_rx) = mpsc::channel::<ControllerCommand>(64);

        let pending = PendingRequests::<ControllerResponse>::new();

        // 发送认证请求作为首条消息
        let register_msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::Register(oxiproxy::NodeRegisterRequest {
                token: token.to_string(),
                tunnel_protocol: tunnel_protocol.to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
        };
        tx.send(register_msg).await
            .map_err(|_| anyhow!("发送认证消息失败"))?;

        // 建立 gRPC 流
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
        let response = client.agent_server_channel(outbound).await
            .map_err(|e| anyhow!("建立 gRPC 流失败: {}", e))?;

        let mut inbound = response.into_inner();

        // 读取认证响应
        let first_msg = inbound.next().await
            .ok_or_else(|| anyhow!("未收到认证响应"))?
            .map_err(|e| anyhow!("读取认证响应失败: {}", e))?;

        let register_resp = match first_msg.payload {
            Some(ControllerPayload::RegisterResponse(resp)) => resp,
            _ => return Err(anyhow!("首条响应不是认证响应")),
        };

        let node_id = register_resp.node_id;
        let tunnel_port = register_resp.tunnel_port as u16;
        let authoritative_protocol = if register_resp.tunnel_protocol.is_empty() {
            tunnel_protocol.to_string()
        } else {
            register_resp.tunnel_protocol.clone()
        };
        info!("gRPC 连接认证成功: 节点 #{} ({}), 隧道协议: {}, 隧道端口: {}", node_id, register_resp.node_name, authoritative_protocol, tunnel_port);
        let speed_limit = register_resp.speed_limit;

        // 提取证书数据
        let node_certificate = register_resp.certificate.map(|cert| {
            info!("✅ 收到节点证书（指纹: {}）", cert.fingerprint);
            NodeCertificate {
                cert_pem: cert.cert_pem,
                key_pem: cert.key_pem,
                fingerprint: cert.fingerprint,
            }
        });

        let shared_sender = SharedGrpcSender::new(tx.clone());
        let shared_pending = SharedPendingRequests::new(pending.clone());

        let grpc_client = Arc::new(Self {
            shared_sender,
            shared_pending,
            node_id: RwLock::new(node_id),
        });

        // 启动消息接收循环
        let pending_clone = pending.clone();
        let cmd_tx_clone = cmd_tx.clone();

        tokio::spawn(async move {
            Self::message_loop(inbound, pending_clone, cmd_tx_clone, tx, node_id).await;
        });

        // 启动心跳
        let heartbeat_sender = grpc_client.shared_sender.clone();
        tokio::spawn(async move {
            Self::shared_heartbeat_loop(heartbeat_sender).await;
        });

        Ok((grpc_client, cmd_rx, authoritative_protocol, speed_limit, tunnel_port, node_certificate))
    }

    /// 重连 Controller（复用已有的 SharedGrpcSender 和 SharedPendingRequests）
    ///
    /// 返回 (命令接收器, Controller 下发的权威隧道协议, 速度限制, 隧道端口, 节点证书)
    pub async fn reconnect(
        self: &Arc<Self>,
        controller_url: &str,
        token: &str,
        tunnel_protocol: &str,
        tls_ca_cert: Option<&[u8]>,
    ) -> Result<(mpsc::Receiver<ControllerCommand>, String, Option<i64>, u16, Option<NodeCertificate>)> {
        let mut endpoint = Channel::from_shared(controller_url.to_string())?;

        if controller_url.starts_with("https://") {
            // 从 URL 中提取域名用于 SNI
            let domain = controller_url
                .trim_start_matches("https://")
                .split(':')
                .next()
                .ok_or_else(|| anyhow!("无法从 URL 提取域名"))?;

            let mut tls_config = ClientTlsConfig::new()
                .domain_name(domain)
                .with_webpki_roots();

            if let Some(ca_pem) = tls_ca_cert {
                tls_config = tls_config.ca_certificate(
                    tonic::transport::Certificate::from_pem(ca_pem)
                );
            }

            endpoint = endpoint.tls_config(tls_config)
                .map_err(|e| anyhow!("TLS 配置失败: {}", e))?;
        }

        let channel = endpoint.connect()
            .await
            .map_err(|e| anyhow!("重连 Controller gRPC 失败: {}", e))?;

        let mut client = AgentServerServiceClient::new(channel);

        // 创建新的双向流
        let (tx, rx) = mpsc::channel::<oxiproxy::AgentServerMessage>(256);
        let (cmd_tx, cmd_rx) = mpsc::channel::<ControllerCommand>(64);

        let pending = PendingRequests::<ControllerResponse>::new();

        // 发送认证请求
        let register_msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::Register(oxiproxy::NodeRegisterRequest {
                token: token.to_string(),
                tunnel_protocol: tunnel_protocol.to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            })),
        };
        tx.send(register_msg).await
            .map_err(|_| anyhow!("发送认证消息失败"))?;

        // 建立 gRPC 流
        let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
        let response = client.agent_server_channel(outbound).await
            .map_err(|e| anyhow!("建立 gRPC 流失败: {}", e))?;

        let mut inbound = response.into_inner();

        // 读取认证响应
        let first_msg = inbound.next().await
            .ok_or_else(|| anyhow!("未收到认证响应"))?
            .map_err(|e| anyhow!("读取认证响应失败: {}", e))?;

        let register_resp = match first_msg.payload {
            Some(ControllerPayload::RegisterResponse(resp)) => resp,
            _ => return Err(anyhow!("首条响应不是认证响应")),
        };

        let node_id = register_resp.node_id;
        let tunnel_port = register_resp.tunnel_port as u16;
        let authoritative_protocol = if register_resp.tunnel_protocol.is_empty() {
            tunnel_protocol.to_string()
        } else {
            register_resp.tunnel_protocol.clone()
        };
        info!("gRPC 重连认证成功: 节点 #{} ({}), 隧道协议: {}, 隧道端口: {}", node_id, register_resp.node_name, authoritative_protocol, tunnel_port);
        let speed_limit = register_resp.speed_limit;

        // 提取证书数据
        let node_certificate = register_resp.certificate.map(|cert| {
            info!("✅ 重连后收到节点证书（指纹: {}）", cert.fingerprint);
            NodeCertificate {
                cert_pem: cert.cert_pem,
                key_pem: cert.key_pem,
                fingerprint: cert.fingerprint,
            }
        });

        // 热替换 sender 和 pending
        self.shared_sender.replace(tx.clone()).await;
        self.shared_pending.replace(pending.clone()).await;
        *self.node_id.write().await = node_id;

        // 启动新的消息接收循环
        let pending_clone = pending.clone();
        let cmd_tx_clone = cmd_tx.clone();

        tokio::spawn(async move {
            Self::message_loop(inbound, pending_clone, cmd_tx_clone, tx, node_id).await;
        });

        // 启动新的心跳（旧的会因为 sender 被替换而自动停止）
        let heartbeat_sender = self.shared_sender.clone();
        tokio::spawn(async move {
            Self::shared_heartbeat_loop(heartbeat_sender).await;
        });

        Ok((cmd_rx, authoritative_protocol, speed_limit, tunnel_port, node_certificate))
    }

    /// 消息接收循环
    async fn message_loop(
        mut inbound: tonic::Streaming<oxiproxy::ControllerToAgentMessage>,
        pending: PendingRequests<ControllerResponse>,
        cmd_tx: mpsc::Sender<ControllerCommand>,
        _tx: GrpcSender,
        node_id: i64,
    ) {
        while let Some(result) = inbound.next().await {
            let msg = match result {
                Ok(m) => m,
                Err(e) => {
                    error!("gRPC 流错误: {}", e);
                    break;
                }
            };

            let payload = match msg.payload {
                Some(p) => p,
                None => continue,
            };

            match payload {
                ControllerPayload::HeartbeatResponse(_) => {
                    // 心跳响应，忽略
                }

                ControllerPayload::ValidateTokenResponse(resp) => {
                    let rid = resp.request_id.clone();
                    pending.complete(&rid, ControllerResponse::ValidateToken(resp)).await;
                }

                ControllerPayload::ClientOnlineResponse(resp) => {
                    let rid = resp.request_id.clone();
                    pending.complete(&rid, ControllerResponse::ClientOnline(resp)).await;
                }

                ControllerPayload::TrafficLimitResponse(resp) => {
                    let rid = resp.request_id.clone();
                    pending.complete(&rid, ControllerResponse::TrafficLimit(resp)).await;
                }

                ControllerPayload::GetClientProxiesResponse(resp) => {
                    let rid = resp.request_id.clone();
                    pending.complete(&rid, ControllerResponse::GetClientProxies(resp)).await;
                }

                ControllerPayload::TrafficReportResponse(_resp) => {
                    // 流量上报是 fire-and-forget，无需关联响应
                }

                // Controller 主动下发的指令
                ControllerPayload::StartProxy(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::StartProxy {
                        request_id: cmd.request_id,
                        client_id: cmd.client_id,
                        proxy_id: cmd.proxy_id,
                    }).await;
                }

                ControllerPayload::StopProxy(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::StopProxy {
                        request_id: cmd.request_id,
                        client_id: cmd.client_id,
                        proxy_id: cmd.proxy_id,
                    }).await;
                }

                ControllerPayload::GetStatus(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::GetStatus {
                        request_id: cmd.request_id,
                    }).await;
                }

                ControllerPayload::GetClientLogs(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::GetClientLogs {
                        request_id: cmd.request_id,
                        client_id: cmd.client_id,
                        count: cmd.count,
                    }).await;
                }

                ControllerPayload::GetNodeLogs(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::GetNodeLogs {
                        request_id: cmd.request_id,
                        lines: cmd.lines,
                    }).await;
                }

                ControllerPayload::UpdateProtocol(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::UpdateProtocol {
                        request_id: cmd.request_id,
                        tunnel_protocol: cmd.tunnel_protocol,
                    }).await;
                }

                ControllerPayload::UpdateSpeedLimit(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::UpdateSpeedLimit {
                        request_id: cmd.request_id,
                        speed_limit: cmd.speed_limit,
                    }).await;
                }

                ControllerPayload::SoftwareUpdate(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::SoftwareUpdate {
                        request_id: cmd.request_id,
                    }).await;
                }

                ControllerPayload::UpdateCertificate(cmd) => {
                    let _ = cmd_tx.send(ControllerCommand::UpdateCertificate {
                        request_id: cmd.request_id,
                        certificate: cmd.certificate,
                    }).await;
                }

                _ => {
                    warn!("收到未知的 Controller 消息类型");
                }
            }
        }

        warn!("节点 #{} gRPC 连接断开", node_id);
    }

    /// 心跳循环（使用 SharedGrpcSender）
    async fn shared_heartbeat_loop(sender: SharedGrpcSender) {
        let mut interval = tokio::time::interval(Duration::from_secs(15));
        interval.tick().await; // 跳过首次

        loop {
            interval.tick().await;

            let msg = oxiproxy::AgentServerMessage {
                payload: Some(AgentPayload::Heartbeat(oxiproxy::Heartbeat {
                    timestamp: chrono::Utc::now().timestamp(),
                })),
            };

            if sender.send(msg).await.is_err() {
                warn!("心跳发送失败，连接可能已断开");
                break;
            }
        }
    }

    /// 获取节点 ID
    pub async fn node_id(&self) -> i64 {
        *self.node_id.read().await
    }

    /// 获取共享发送器（供 auth provider 和 traffic manager 使用）
    pub fn shared_sender(&self) -> &SharedGrpcSender {
        &self.shared_sender
    }

    /// 获取共享 pending requests（供 auth provider 使用）
    pub fn shared_pending(&self) -> &SharedPendingRequests {
        &self.shared_pending
    }

    /// 发送命令响应到 Controller
    pub async fn send_response(&self, response: oxiproxy::AgentServerResponse) -> Result<()> {
        let msg = oxiproxy::AgentServerMessage {
            payload: Some(AgentPayload::Response(response)),
        };
        self.shared_sender.send(msg).await
            .map_err(|_| anyhow!("发送响应失败"))?;
        Ok(())
    }
}

/// Controller 下发的命令
pub enum ControllerCommand {
    StartProxy {
        request_id: String,
        client_id: String,
        proxy_id: i64,
    },
    StopProxy {
        request_id: String,
        client_id: String,
        proxy_id: i64,
    },
    GetStatus {
        request_id: String,
    },
    GetClientLogs {
        request_id: String,
        client_id: String,
        count: u32,
    },
    GetNodeLogs {
        request_id: String,
        lines: u32,
    },
    UpdateProtocol {
        request_id: String,
        tunnel_protocol: String,
    },
    UpdateSpeedLimit {
        request_id: String,
        speed_limit: i64,
    },
    SoftwareUpdate {
        request_id: String,
    },
    UpdateCertificate {
        request_id: String,
        certificate: Option<oxiproxy::NodeCertificate>,
    },
}

/// 命令处理器：处理 Controller 下发的命令并发送响应
pub async fn handle_controller_commands(
    mut cmd_rx: mpsc::Receiver<ControllerCommand>,
    grpc_client: Arc<AgentGrpcClient>,
    proxy_control: Arc<dyn ProxyControl>,
    tunnel_manager: Arc<super::tunnel_manager::TunnelManager>,
    speed_limiter: Arc<super::speed_limiter::SpeedLimiter>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        let grpc = grpc_client.clone();
        let control = proxy_control.clone();
        let tm = tunnel_manager.clone();
        let sl = speed_limiter.clone();

        tokio::spawn(async move {
            match cmd {
                ControllerCommand::StartProxy { request_id, client_id, proxy_id } => {
                    let result = control.start_proxy(&client_id, proxy_id).await;
                    let ack = match result {
                        Ok(()) => oxiproxy::CommandAck { success: true, error: None },
                        Err(e) => oxiproxy::CommandAck { success: false, error: Some(e.to_string()) },
                    };
                    let resp = oxiproxy::AgentServerResponse {
                        request_id,
                        result: Some(AgentResult::CommandAck(ack)),
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::StopProxy { request_id, client_id, proxy_id } => {
                    let result = control.stop_proxy(&client_id, proxy_id).await;
                    let ack = match result {
                        Ok(()) => oxiproxy::CommandAck { success: true, error: None },
                        Err(e) => oxiproxy::CommandAck { success: false, error: Some(e.to_string()) },
                    };
                    let resp = oxiproxy::AgentServerResponse {
                        request_id,
                        result: Some(AgentResult::CommandAck(ack)),
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::GetStatus { request_id } => {
                    let result = control.get_server_status().await;
                    let resp = match result {
                        Ok(status) => {
                            let clients: Vec<oxiproxy::ConnectedClient> = status.connected_clients
                                .into_iter()
                                .map(|c| oxiproxy::ConnectedClient {
                                    client_id: c.client_id,
                                    remote_address: c.remote_address,
                                    protocol: c.protocol,
                                })
                                .collect();
                            oxiproxy::AgentServerResponse {
                                request_id,
                                result: Some(AgentResult::ServerStatus(oxiproxy::ServerStatus {
                                    connected_clients: clients,
                                    active_proxy_count: status.active_proxy_count as u32,
                                })),
                            }
                        }
                        Err(e) => oxiproxy::AgentServerResponse {
                            request_id,
                            result: Some(AgentResult::CommandAck(oxiproxy::CommandAck {
                                success: false,
                                error: Some(e.to_string()),
                            })),
                        },
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::GetClientLogs { request_id, client_id, count } => {
                    let result = control.fetch_client_logs(&client_id, count as u16).await;
                    let resp = match result {
                        Ok(logs) => {
                            let entries: Vec<oxiproxy::LogEntry> = logs
                                .into_iter()
                                .map(|l| oxiproxy::LogEntry {
                                    timestamp: l.timestamp,
                                    level: l.level,
                                    message: l.message,
                                })
                                .collect();
                            oxiproxy::AgentServerResponse {
                                request_id,
                                result: Some(AgentResult::ClientLogs(oxiproxy::ClientLogsResponse {
                                    logs: entries,
                                })),
                            }
                        }
                        Err(e) => oxiproxy::AgentServerResponse {
                            request_id,
                            result: Some(AgentResult::CommandAck(oxiproxy::CommandAck {
                                success: false,
                                error: Some(e.to_string()),
                            })),
                        },
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::GetNodeLogs { request_id, lines } => {
                    // 读取节点日志文件
                    let logs = read_node_logs(lines).await;
                    let resp = match logs {
                        Ok(entries) => {
                            let log_entries: Vec<oxiproxy::LogEntry> = entries
                                .into_iter()
                                .map(|l| oxiproxy::LogEntry {
                                    timestamp: l.timestamp,
                                    level: l.level,
                                    message: l.message,
                                })
                                .collect();
                            oxiproxy::AgentServerResponse {
                                request_id,
                                result: Some(AgentResult::NodeLogs(oxiproxy::NodeLogsResponse {
                                    logs: log_entries,
                                })),
                            }
                        }
                        Err(e) => oxiproxy::AgentServerResponse {
                            request_id,
                            result: Some(AgentResult::CommandAck(oxiproxy::CommandAck {
                                success: false,
                                error: Some(e.to_string()),
                            })),
                        },
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::UpdateProtocol { request_id, tunnel_protocol } => {
                    let result = tm.switch_protocol(&tunnel_protocol).await;
                    let ack = match result {
                        Ok(()) => oxiproxy::CommandAck { success: true, error: None },
                        Err(e) => oxiproxy::CommandAck { success: false, error: Some(e.to_string()) },
                    };
                    let resp = oxiproxy::AgentServerResponse {
                        request_id,
                        result: Some(AgentResult::CommandAck(ack)),
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::UpdateSpeedLimit { request_id, speed_limit } => {
                    sl.update_rate(speed_limit as u64);
                    info!("速度限制已更新: {} bytes/s", speed_limit);
                    let resp = oxiproxy::AgentServerResponse {
                        request_id,
                        result: Some(AgentResult::CommandAck(oxiproxy::CommandAck {
                            success: true,
                            error: None,
                        })),
                    };
                    let _ = grpc.send_response(resp).await;
                }

                ControllerCommand::SoftwareUpdate { request_id } => {
                    info!("收到远程软件更新指令，开始更新...");
                    let update_result = tokio::task::spawn_blocking(perform_node_self_update).await;
                    let (success, error_msg, new_ver) = match update_result {
                        Ok(Ok(v)) => (true, None, Some(v)),
                        Ok(Err(e)) => (false, Some(e.to_string()), None),
                        Err(e) => (false, Some(e.to_string()), None),
                    };
                    let resp = oxiproxy::AgentServerResponse {
                        request_id,
                        result: Some(AgentResult::SoftwareUpdate(oxiproxy::SoftwareUpdateResponse {
                            success,
                            error: error_msg,
                            new_version: new_ver,
                        })),
                    };
                    let _ = grpc.send_response(resp).await;
                    if success {
                        info!("软件更新成功，3秒后重启...");
                        tokio::time::sleep(Duration::from_secs(3)).await;
                        restart_self();
                    }
                }

                ControllerCommand::UpdateCertificate { request_id, certificate } => {
                    info!("🔄 收到证书更新指令");
                    let result = if let Some(cert) = certificate {
                        // 通过 ProxyControl 获取 ProxyServer 并更新证书
                        match control.update_certificate(cert.cert_pem, cert.key_pem).await {
                            Ok(()) => {
                                info!("✅ 证书更新成功");
                                (true, None)
                            }
                            Err(e) => {
                                error!("❌ 证书更新失败: {}", e);
                                (false, Some(e.to_string()))
                            }
                        }
                    } else {
                        error!("❌ 证书更新失败: 未提供证书数据");
                        (false, Some("未提供证书数据".to_string()))
                    };

                    let resp = oxiproxy::AgentServerResponse {
                        request_id,
                        result: Some(AgentResult::UpdateCertificate(oxiproxy::UpdateCertificateResponse {
                            success: result.0,
                            error: result.1,
                        })),
                    };
                    let _ = grpc.send_response(resp).await;
                }
            }
        });
    }
}

/// 读取节点日志（从内存缓冲区或日志文件）
async fn read_node_logs(lines: u32) -> Result<Vec<LogEntry>> {
    // 优先从内存缓冲区读取
    if let Some(buffer) = super::node_logs::get_global_log_buffer() {
        let logs = buffer.get_last(lines as usize);
        if !logs.is_empty() {
            return Ok(logs);
        }
    }

    // 如果内存缓冲区不可用或为空，尝试从日志文件读取（守护进程模式）
    #[cfg(unix)]
    {
        use tokio::process::Command;

        let log_file = std::env::var("OXIPROXY_LOG_FILE")
            .unwrap_or_else(|_| "/var/log/oxiproxy-agent.log".to_string());

        let output = Command::new("tail")
            .arg("-n")
            .arg(lines.to_string())
            .arg(&log_file)
            .output()
            .await;

        if let Ok(out) = output {
            if out.status.success() {
                let content = String::from_utf8_lossy(&out.stdout);
                let logs: Vec<LogEntry> = content.lines()
                    .map(|line| {
                        let parts: Vec<&str> = line.splitn(3, ' ').collect();
                        if parts.len() >= 3 {
                            LogEntry {
                                timestamp: parts[0].to_string(),
                                level: parts[1].to_string(),
                                message: parts[2].to_string(),
                            }
                        } else {
                            LogEntry {
                                timestamp: chrono::Utc::now().to_rfc3339(),
                                level: "INFO".to_string(),
                                message: line.to_string(),
                            }
                        }
                    })
                    .collect();

                if !logs.is_empty() {
                    return Ok(logs);
                }
            }
        }
    }

    // 如果所有方法都失败，返回提示信息
    Ok(vec![LogEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: "INFO".to_string(),
        message: "日志系统正在初始化，暂无日志记录".to_string(),
    }])
}

/// 重启当前进程（用于自更新后重启）
///
/// - Unix: 使用 exec 替换当前进程（保持 PID 不变，适用于 daemon 模式）
/// - Windows: 启动新的分离进程后退出
fn restart_self() -> ! {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            error!("无法获取当前可执行文件路径: {}", e);
            std::process::exit(1);
        }
    };

    let original_args: Vec<String> = std::env::args().collect();

    // 构建重启参数：将 "daemon" 替换为 "start"，过滤掉 daemon 特有的 --pid-file 参数
    let mut restart_args: Vec<String> = Vec::new();
    let mut skip_next = false;

    for (i, arg) in original_args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }

        // 第一个参数是子命令，daemon -> start（已在 daemon 上下文中，无需再次 fork）
        if i == 1 && arg == "daemon" {
            restart_args.push("start".to_string());
            continue;
        }

        // 跳过 daemon 特有的 --pid-file 参数及其值
        if arg == "--pid-file" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--pid-file=") {
            continue;
        }

        restart_args.push(arg.clone());
    }

    info!("重启进程: {:?} {:?}", exe, restart_args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec 替换当前进程，保持 PID 不变
        let err = std::process::Command::new(&exe).args(&restart_args).exec();
        error!("exec 重启失败: {}", err);
        std::process::exit(1);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        match std::process::Command::new(&exe)
            .args(&restart_args)
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(child) => info!("新进程已启动 (PID: {})", child.id()),
            Err(e) => error!("重启失败: {}", e),
        }
        std::process::exit(0);
    }
}

/// 执行节点自更新（阻塞操作，需在 spawn_blocking 中调用）
fn perform_node_self_update() -> anyhow::Result<String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("oxiproxy")
        .repo_name("oxiproxy")
        .bin_name("node")
        .identifier("node")
        .show_download_progress(false)
        .current_version(env!("CARGO_PKG_VERSION"))
        .no_confirm(true)
        .build()?
        .update()?;

    match status {
        self_update::Status::UpToDate(v) => Ok(v),
        self_update::Status::Updated(v) => Ok(v),
    }
}
