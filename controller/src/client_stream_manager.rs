//! Agent Client 流管理器
//!
//! 管理所有已连接的 Agent Client gRPC 流，
//! 当代理配置变更时推送 ProxyListUpdate 通知。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};

use common::grpc::oxiproxy;
use common::grpc::pending_requests::PendingRequests;
use common::KcpConfig;
use common::protocol::control::LogEntry;

use crate::entity::{Client, Node, Proxy, proxy, node};
use crate::migration::get_connection;
use crate::certificate_manager::CertificateManager;

/// 单个客户端的流连接
struct ClientStream {
    tx: mpsc::Sender<Result<oxiproxy::ControllerToClientMessage, tonic::Status>>,
    pending: PendingRequests<oxiproxy::AgentClientResponse>,
}

/// 管理已连接的 Agent Client 流
#[derive(Clone)]
pub struct ClientStreamManager {
    /// client_id -> stream
    streams: Arc<RwLock<HashMap<i64, ClientStream>>>,
}

impl ClientStreamManager {
    pub fn new() -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 注册一个 Agent Client 流
    pub async fn register(
        &self,
        client_id: i64,
        tx: mpsc::Sender<Result<oxiproxy::ControllerToClientMessage, tonic::Status>>,
    ) {
        info!("Agent Client #{} 已连接", client_id);
        let stream = ClientStream {
            tx,
            pending: PendingRequests::new(),
        };
        self.streams.write().await.insert(client_id, stream);
    }

    /// 移除一个 Agent Client 流
    pub async fn unregister(&self, client_id: i64) {
        info!("Agent Client #{} 已断开", client_id);
        self.streams.write().await.remove(&client_id);
    }

    /// 通知指定客户端代理配置已变更
    pub async fn notify_proxy_change(&self, client_id_str: &str) {
        let client_id: i64 = match client_id_str.parse() {
            Ok(id) => id,
            Err(_) => return,
        };

        let update = match self.build_proxy_list_update(client_id).await {
            Ok(u) => u,
            Err(e) => {
                error!("构建代理列表更新失败: {}", e);
                return;
            }
        };

        let streams = self.streams.read().await;
        if let Some(stream) = streams.get(&client_id) {
            let msg = oxiproxy::ControllerToClientMessage {
                payload: Some(oxiproxy::controller_to_client_message::Payload::ProxyUpdate(update)),
            };
            if let Err(e) = stream.tx.send(Ok(msg)).await {
                error!("推送代理更新到 Client #{} 失败: {}", client_id, e);
            } else {
                debug!("已推送代理更新到 Client #{}", client_id);
            }
        }
    }

    /// 通知某个节点上的所有客户端刷新配置
    pub async fn notify_clients_for_node(&self, node_id: i64) {
        let db = get_connection().await;

        // 查询该节点上所有启用的代理
        let proxies = match Proxy::find()
            .filter(proxy::Column::NodeId.eq(node_id))
            .filter(proxy::Column::Enabled.eq(true))
            .all(db)
            .await
        {
            Ok(p) => p,
            Err(e) => {
                error!("查询节点 #{} 的代理失败: {}", node_id, e);
                return;
            }
        };

        // 收集所有不重复的 client_id
        let mut client_ids = std::collections::HashSet::new();
        for proxy in proxies {
            client_ids.insert(proxy.client_id);
        }

        // 逐个通知客户端
        for client_id_str in client_ids {
            self.notify_proxy_change(&client_id_str).await;
        }
    }

    /// 健康检查所有客户端
    pub async fn check_all_clients(&self) -> Vec<(i64, bool)> {
        let db = get_connection().await;
        let all_clients = match Client::find().all(db).await {
            Ok(clients) => clients,
            Err(e) => {
                error!("查询客户端列表失败: {}", e);
                return vec![];
            }
        };

        let streams = self.streams.read().await;

        all_clients
            .into_iter()
            .map(|client| {
                let is_online = streams.contains_key(&client.id);
                (client.id, is_online)
            })
            .collect()
    }

    /// 完成一个待处理的请求（由 AgentClientResponse 触发）
    pub async fn complete_pending_request(&self, client_id: i64, response: &oxiproxy::AgentClientResponse) {
        let streams = self.streams.read().await;
        if let Some(stream) = streams.get(&client_id) {
            stream.pending.complete(&response.request_id, response.clone()).await;
        }
    }

    /// 获取客户端日志
    pub async fn fetch_client_logs(&self, client_id: i64, count: u16) -> anyhow::Result<Vec<LogEntry>> {
        let (request_id, rx, tx_clone) = {
            let streams = self.streams.read().await;
            let stream = streams.get(&client_id)
                .ok_or_else(|| anyhow::anyhow!("客户端 #{} 未连接", client_id))?;

            let (request_id, rx) = stream.pending.register().await;
            (request_id, rx, stream.tx.clone())
        };

        let msg = oxiproxy::ControllerToClientMessage {
            payload: Some(oxiproxy::controller_to_client_message::Payload::GetLogs(
                oxiproxy::GetClientLogsDirectCommand {
                    request_id: request_id.clone(),
                    count: count as u32,
                },
            )),
        };

        tx_clone.send(Ok(msg)).await
            .map_err(|_| anyhow::anyhow!("发送日志请求到客户端 #{} 失败", client_id))?;

        let resp = PendingRequests::wait(rx, Duration::from_secs(10)).await?;

        match resp.result {
            Some(oxiproxy::agent_client_response::Result::ClientLogs(logs)) => {
                Ok(logs.logs.into_iter().map(|l| LogEntry {
                    timestamp: l.timestamp,
                    level: l.level,
                    message: l.message,
                }).collect())
            }
            _ => Err(anyhow::anyhow!("收到意外的响应类型")),
        }
    }

    /// 向客户端发送软件更新指令
    pub async fn send_software_update(&self, client_id: i64) -> anyhow::Result<oxiproxy::SoftwareUpdateResponse> {
        let (request_id, rx, tx_clone) = {
            let streams = self.streams.read().await;
            let stream = streams.get(&client_id)
                .ok_or_else(|| anyhow::anyhow!("客户端 #{} 未连接", client_id))?;

            let (request_id, rx) = stream.pending.register().await;
            (request_id, rx, stream.tx.clone())
        };

        let msg = oxiproxy::ControllerToClientMessage {
            payload: Some(oxiproxy::controller_to_client_message::Payload::SoftwareUpdate(
                oxiproxy::SoftwareUpdateCommand {
                    request_id: request_id.clone(),
                },
            )),
        };

        tx_clone.send(Ok(msg)).await
            .map_err(|_| anyhow::anyhow!("发送更新请求到客户端 #{} 失败", client_id))?;

        let resp = PendingRequests::wait(rx, Duration::from_secs(120)).await?;

        match resp.result {
            Some(oxiproxy::agent_client_response::Result::SoftwareUpdate(update_resp)) => {
                Ok(update_resp)
            }
            _ => Err(anyhow::anyhow!("收到意外的响应类型")),
        }
    }

    /// 构建代理列表更新消息
    pub async fn build_proxy_list_update(&self, client_id: i64) -> anyhow::Result<oxiproxy::ProxyListUpdate> {
        let db = get_connection().await;

        // 查询客户端
        let client_model = Client::find_by_id(client_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow::anyhow!("客户端 #{} 不存在", client_id))?;

        // 查询所有已启用代理
        let proxies = Proxy::find()
            .filter(proxy::Column::ClientId.eq(client_id.to_string()))
            .filter(proxy::Column::Enabled.eq(true))
            .all(db)
            .await?;

        // 按 node_id 分组（只使用 proxy.node_id）
        let mut node_proxy_map: HashMap<i64, Vec<oxiproxy::ProxyInfo>> = HashMap::new();
        for p in &proxies {
            let nid = match p.node_id {
                Some(id) => id,
                None => continue, // 跳过没有指定节点的代理
            };

            node_proxy_map
                .entry(nid)
                .or_default()
                .push(oxiproxy::ProxyInfo {
                    proxy_id: p.id,
                    name: p.name.clone(),
                    proxy_type: p.proxy_type.clone(),
                    local_ip: p.local_ip.clone(),
                    local_port: p.local_port as i32,
                    remote_port: p.remote_port as i32,
                    enabled: p.enabled,
                });
        }

        // 查询节点信息
        let node_ids: Vec<i64> = node_proxy_map.keys().cloned().collect();
        let nodes = if node_ids.is_empty() {
            vec![]
        } else {
            Node::find()
                .filter(node::Column::Id.is_in(node_ids))
                .all(db)
                .await?
        };

        let mut server_groups = Vec::new();
        for n in nodes {
            if let Some(proxy_list) = node_proxy_map.remove(&n.id) {
                let kcp = n.kcp_config
                    .as_deref()
                    .and_then(|s| serde_json::from_str::<KcpConfig>(s).ok())
                    .map(|k| oxiproxy::GrpcKcpConfig {
                        nodelay: k.nodelay,
                        interval: k.interval,
                        resend: k.resend,
                        nc: k.nc,
                    });

                // 获取节点证书指纹
                let cert_fingerprints = match CertificateManager::get_node_fingerprint(n.id).await {
                    Ok(Some(fingerprint)) => {
                        debug!("为节点 #{} 添加证书指纹: {}", n.id, fingerprint);
                        vec![fingerprint]
                    }
                    Ok(None) => {
                        debug!("节点 #{} 暂无证书指纹", n.id);
                        vec![]
                    }
                    Err(e) => {
                        error!("获取节点 #{} 证书指纹失败: {}", n.id, e);
                        vec![]
                    }
                };

                server_groups.push(oxiproxy::ServerProxyGroup {
                    node_id: n.id,
                    server_addr: n.tunnel_addr,
                    server_port: n.tunnel_port as u32,
                    protocol: n.tunnel_protocol,
                    kcp,
                    proxies: proxy_list,
                    cert_fingerprints,
                });
            }
        }

        Ok(oxiproxy::ProxyListUpdate {
            client_id: client_model.id,
            client_name: client_model.name,
            server_groups,
        })
    }
}
