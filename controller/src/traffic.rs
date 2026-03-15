use anyhow::Result;
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, NotSet};
use sea_orm::sea_query::{OnConflict, Expr};
use std::collections::HashMap;
use tracing::{debug, error, info};
use tokio::sync::mpsc;
use std::time::Duration;

use crate::entity::{proxy, client, user, node, traffic_daily, Proxy, Client, User, Node, TrafficDaily};
use crate::migration::get_connection;

struct TrafficEvent {
    proxy_id: i64,
    client_id: i64,
    user_id: Option<i64>,
    bytes_sent: i64,
    bytes_received: i64,
}

/// 流量统计管理器
#[derive(Clone)]
pub struct TrafficManager {
    sender: mpsc::Sender<TrafficEvent>,
}

impl TrafficManager {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<TrafficEvent>(10000);

        tokio::spawn(async move {
            let mut buffer: HashMap<(i64, i64, Option<i64>), (i64, i64)> = HashMap::new();
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        let key = (event.proxy_id, event.client_id, event.user_id);
                        let entry = buffer.entry(key).or_insert((0, 0));
                        entry.0 += event.bytes_sent;
                        entry.1 += event.bytes_received;

                        // 防止内存积压，如果积压太多则立即刷新
                        if buffer.len() > 1000 {
                            Self::flush_buffer(&mut buffer).await;
                        }
                    }
                    _ = interval.tick() => {
                        if !buffer.is_empty() {
                            Self::flush_buffer(&mut buffer).await;
                        }
                    }
                }
            }
        });

        Self { sender: tx }
    }

    async fn flush_buffer(buffer: &mut HashMap<(i64, i64, Option<i64>), (i64, i64)>) {
        let db = get_connection().await;
        let today = Utc::now().format("%Y-%m-%d").to_string();
        let now = Utc::now().naive_utc();

        let count = buffer.len();
        debug!("🔄 正在批量写入流量统计数据: {} 条聚合记录", count);

        // 用于聚合节点流量
        let mut node_traffic: HashMap<i64, (i64, i64)> = HashMap::new();

        for ((proxy_id, client_id, _user_id), (bytes_sent, bytes_received)) in buffer.drain() {
            if bytes_sent == 0 && bytes_received == 0 {
                continue;
            }

            // 1. 更新代理流量，同时收集 node_id（代理已删除则跳过，避免外键约束失败）
            let Ok(Some(proxy)) = Proxy::find_by_id(proxy_id).one(db).await else {
                debug!("代理 #{} 已不存在，跳过流量记录", proxy_id);
                continue;
            };

            // 收集节点流量
            if let Some(nid) = proxy.node_id {
                let entry = node_traffic.entry(nid).or_insert((0, 0));
                entry.0 += bytes_sent;
                entry.1 += bytes_received;
            }

            let mut proxy_active: proxy::ActiveModel = proxy.into();
            // Safety: proxy_active 由 Model 转换而来，字段为 Unchanged(v)，unwrap 安全
            proxy_active.total_bytes_sent = Set(proxy_active.total_bytes_sent.unwrap() + bytes_sent);
            proxy_active.total_bytes_received = Set(proxy_active.total_bytes_received.unwrap() + bytes_received);
            proxy_active.updated_at = Set(now);
            if let Err(e) = proxy_active.update(db).await {
                error!("更新代理流量失败: {}", e);
            }

            // 2. 查询客户端（用于每日统计和客户端流量更新）
            let client_opt = Client::find_by_id(client_id).one(db).await.ok().flatten();

            // 3. 更新每日流量统计（仅在客户端也存在时插入，避免外键约束失败）
            if client_opt.is_some() {
                let daily = traffic_daily::ActiveModel {
                    id: NotSet,
                    proxy_id: Set(proxy_id),
                    client_id: Set(client_id),
                    bytes_sent: Set(bytes_sent),
                    bytes_received: Set(bytes_received),
                    date: Set(today.clone()),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                let on_conflict = OnConflict::columns([
                    traffic_daily::Column::ProxyId,
                    traffic_daily::Column::Date,
                ])
                .value(
                    traffic_daily::Column::BytesSent,
                    Expr::col(traffic_daily::Column::BytesSent).add(bytes_sent),
                )
                .value(
                    traffic_daily::Column::BytesReceived,
                    Expr::col(traffic_daily::Column::BytesReceived).add(bytes_received),
                )
                .value(traffic_daily::Column::UpdatedAt, now)
                .to_owned();
                if let Err(e) = TrafficDaily::insert(daily)
                    .on_conflict(on_conflict)
                    .exec(db)
                    .await
                {
                    error!("插入/更新每日流量统计失败: {}", e);
                }
            }

            // 4. 更新客户端流量
            if let Some(client) = client_opt {
                let client_user_id = client.user_id;
                let needs_reset = crate::traffic_limiter::should_reset_client_traffic(&client);

                let mut client_active: client::ActiveModel = client.clone().into();

                if needs_reset {
                    client_active.total_bytes_sent = Set(bytes_sent);
                    client_active.total_bytes_received = Set(bytes_received);
                    client_active.is_traffic_exceeded = Set(false);
                    client_active.last_reset_at = Set(Some(now));
                    info!("🔄 客户端 #{} ({}) 流量已自动重置", client_id, client.name);
                } else {
                    // Safety: client_active 由 Model 转换而来，字段为 Unchanged(v)，unwrap 安全
                    client_active.total_bytes_sent = Set(client_active.total_bytes_sent.unwrap() + bytes_sent);
                    client_active.total_bytes_received = Set(client_active.total_bytes_received.unwrap() + bytes_received);
                }

                client_active.updated_at = Set(now);

                if let Err(e) = client_active.update(db).await {
                    error!("更新客户端流量失败: {}", e);
                } else {
                    let new_sent = if needs_reset { bytes_sent } else { client.total_bytes_sent + bytes_sent };
                    let new_received = if needs_reset { bytes_received } else { client.total_bytes_received + bytes_received };

                    // 检查客户端配额
                    if let Some(quota_gb) = client.traffic_quota_gb {
                        let total_used = new_sent + new_received;
                        let quota_bytes = crate::traffic_limiter::gb_to_bytes(quota_gb);
                        if total_used >= quota_bytes && !client.is_traffic_exceeded {
                            if let Ok(Some(c)) = Client::find_by_id(client_id).one(db).await {
                                let mut c_active: client::ActiveModel = c.into();
                                c_active.is_traffic_exceeded = Set(true);
                                c_active.updated_at = Set(now);
                                let _ = c_active.update(db).await;
                                error!("⚠️ 客户端 #{} ({}) 流量配额已用尽: {:.2} GB / {:.2} GB",
                                    client_id, client.name,
                                    crate::traffic_limiter::bytes_to_gb(total_used),
                                    quota_gb);
                            }
                        }
                    }
                }

                // 4. 如果客户端有 user_id，同时更新用户流量
                if let Some(uid) = client_user_id {
                    if let Ok(Some(user)) = User::find_by_id(uid).one(db).await {
                        let needs_reset = crate::traffic_limiter::should_reset_traffic(&user);

                        let mut user_active: user::ActiveModel = user.clone().into();

                        if needs_reset {
                            user_active.total_bytes_sent = Set(bytes_sent);
                            user_active.total_bytes_received = Set(bytes_received);
                            user_active.is_traffic_exceeded = Set(false);
                            user_active.last_reset_at = Set(Some(now));
                            info!("🔄 用户 #{} ({}) 流量已自动重置", uid, user.username);
                        } else {
                            // Safety: user_active 由 Model 转换而来，字段为 Unchanged(v)，unwrap 安全
                            user_active.total_bytes_sent = Set(user_active.total_bytes_sent.unwrap() + bytes_sent);
                            user_active.total_bytes_received = Set(user_active.total_bytes_received.unwrap() + bytes_received);
                        }

                        user_active.updated_at = Set(now);

                        if let Err(e) = user_active.update(db).await {
                            error!("更新用户流量失败: {}", e);
                        } else {
                            let new_sent = if needs_reset { bytes_sent } else { user.total_bytes_sent + bytes_sent };
                            let new_received = if needs_reset { bytes_received } else { user.total_bytes_received + bytes_received };

                            // 检查用户配额
                            if let Some(quota_gb) = user.traffic_quota_gb {
                                let total_used = new_sent + new_received;
                                let quota_bytes = crate::traffic_limiter::gb_to_bytes(quota_gb);
                                if total_used >= quota_bytes && !user.is_traffic_exceeded {
                                    if let Ok(Some(u)) = User::find_by_id(uid).one(db).await {
                                        let mut u_active: user::ActiveModel = u.into();
                                        u_active.is_traffic_exceeded = Set(true);
                                        u_active.updated_at = Set(now);
                                        let _ = u_active.update(db).await;
                                        error!("⚠️ 用户 #{} ({}) 流量配额已用尽: {:.2} GB / {:.2} GB",
                                            uid, user.username,
                                            crate::traffic_limiter::bytes_to_gb(total_used),
                                            quota_gb);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // 5. 批量更新节点流量
        for (nid, (sent, received)) in node_traffic {
            if let Ok(Some(node_model)) = Node::find_by_id(nid).one(db).await {
                let needs_reset = crate::traffic_limiter::should_reset_node_traffic(&node_model);

                let mut node_active: node::ActiveModel = node_model.clone().into();

                let (new_sent, new_received) = if needs_reset {
                    node_active.total_bytes_sent = Set(sent);
                    node_active.total_bytes_received = Set(received);
                    node_active.is_traffic_exceeded = Set(false);
                    node_active.last_reset_at = Set(Some(now));
                    info!("🔄 节点 #{} ({}) 流量已自动重置", nid, node_model.name);
                    (sent, received)
                } else {
                    let ns = node_model.total_bytes_sent + sent;
                    let nr = node_model.total_bytes_received + received;
                    node_active.total_bytes_sent = Set(ns);
                    node_active.total_bytes_received = Set(nr);
                    (ns, nr)
                };

                node_active.updated_at = Set(now);

                if let Err(e) = node_active.update(db).await {
                    error!("更新节点流量失败: {}", e);
                } else {
                    // 检查节点配额
                    if let Some(quota_gb) = node_model.traffic_quota_gb {
                        let total_used = new_sent + new_received;
                        let quota_bytes = crate::traffic_limiter::gb_to_bytes(quota_gb);
                        if total_used >= quota_bytes && !node_model.is_traffic_exceeded {
                            if let Ok(Some(n)) = Node::find_by_id(nid).one(db).await {
                                let mut n_active: node::ActiveModel = n.into();
                                n_active.is_traffic_exceeded = Set(true);
                                n_active.updated_at = Set(now);
                                let _ = n_active.update(db).await;
                                error!("⚠️ 节点 #{} ({}) 流量配额已用尽: {:.2} GB / {:.2} GB",
                                    nid, node_model.name,
                                    crate::traffic_limiter::bytes_to_gb(total_used),
                                    quota_gb);
                            }
                        }
                    }
                }
            }
        }
    }

    /// 实时记录流量统计到数据库 (异步非阻塞)
    pub async fn record_traffic(
        &self,
        proxy_id: i64,
        client_id: i64,
        user_id: Option<i64>,
        bytes_sent: i64,
        bytes_received: i64,
    ) {
        if bytes_sent == 0 && bytes_received == 0 {
            return;
        }

        let event = TrafficEvent {
            proxy_id,
            client_id,
            user_id,
            bytes_sent,
            bytes_received,
        };

        if let Err(e) = self.sender.send(event).await {
            error!("发送流量统计事件失败: {}", e);
        }
    }

    /// 不再需要定时刷新，保留此方法用于兼容
    pub async fn flush_to_database(&self) -> Result<()> {
        Ok(())
    }

    /// 不再需要定时刷新，保留此方法用于兼容
    pub fn start_periodic_flush(self: std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async {})
    }
}

/// 流量统计响应结构
#[derive(Debug, serde::Serialize)]
pub struct TrafficOverview {
    pub total_traffic: TotalTraffic,
    pub by_user: Vec<UserTraffic>,
    pub by_client: Vec<ClientTraffic>,
    pub by_proxy: Vec<ProxyTraffic>,
    pub daily_traffic: Vec<DailyTraffic>,
}

#[derive(Debug, serde::Serialize)]
pub struct TotalTraffic {
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct UserTraffic {
    pub user_id: i64,
    pub username: String,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ClientTraffic {
    pub client_id: i64,
    pub client_name: String,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ProxyTraffic {
    pub proxy_id: i64,
    pub proxy_name: String,
    pub client_id: i64,
    pub client_name: String,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct DailyTraffic {
    pub date: String,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
}

/// 获取流量总览
pub async fn get_traffic_overview(user_id: Option<i64>, days: i64) -> Result<TrafficOverview> {
    let db = get_connection().await;

    let is_admin = if let Some(uid) = user_id {
        if let Some(user) = User::find_by_id(uid).one(db).await? {
            user.is_admin
        } else {
            false
        }
    } else {
        false
    };

    // 获取所有用户流量
    let mut users = Vec::new();
    let mut total_sent = 0i64;
    let mut total_received = 0i64;

    if is_admin {
        let all_users = User::find().all(db).await?;
        for user in all_users {
            let total = user.total_bytes_sent + user.total_bytes_received;
            users.push(UserTraffic {
                user_id: user.id,
                username: user.username,
                total_bytes_sent: user.total_bytes_sent,
                total_bytes_received: user.total_bytes_received,
                total_bytes: total,
            });
        }
    } else if let Some(uid) = user_id {
        if let Some(user) = User::find_by_id(uid).one(db).await? {
            let total = user.total_bytes_sent + user.total_bytes_received;
            total_sent += user.total_bytes_sent;
            total_received += user.total_bytes_received;
            users.push(UserTraffic {
                user_id: user.id,
                username: user.username,
                total_bytes_sent: user.total_bytes_sent,
                total_bytes_received: user.total_bytes_received,
                total_bytes: total,
            });
        }
    }

    // 获取客户端流量
    let mut clients = Vec::new();
    let all_clients = Client::find().all(db).await?;
    for client in all_clients {
        let total = client.total_bytes_sent + client.total_bytes_received;
        if !is_admin {
            // 如果不是管理员，只显示有权限的客户端
            if let Some(uid) = user_id {
                if !has_client_access(db, uid, client.id).await? {
                    continue;
                }
            }
        }
        // 管理员模式下从 client 表统计总流量（避免从 user 表统计导致遗漏无关联用户的流量）
        if is_admin {
            total_sent += client.total_bytes_sent;
            total_received += client.total_bytes_received;
        }
        clients.push(ClientTraffic {
            client_id: client.id,
            client_name: client.name,
            total_bytes_sent: client.total_bytes_sent,
            total_bytes_received: client.total_bytes_received,
            total_bytes: total,
        });
    }

    // 获取代理流量
    let mut proxies = Vec::new();
    let all_proxies = Proxy::find().all(db).await?;
    for proxy in all_proxies {
        let proxy_client_id = match proxy.client_id.parse::<i64>() {
            Ok(id) => id,
            Err(_) => {
                error!("代理 #{} 的 client_id '{}' 无法解析为整数，跳过", proxy.id, proxy.client_id);
                continue;
            }
        };

        let total = proxy.total_bytes_sent + proxy.total_bytes_received;
        if !is_admin {
            // 如果不是管理员，只显示有权限的代理
            if let Some(uid) = user_id {
                if !has_client_access(db, uid, proxy_client_id).await? {
                    continue;
                }
            }
        }

        let client_name = if let Some(client) = Client::find_by_id(proxy_client_id).one(db).await? {
            client.name
        } else {
            String::from("Unknown")
        };

        proxies.push(ProxyTraffic {
            proxy_id: proxy.id,
            proxy_name: proxy.name,
            client_id: proxy_client_id,
            client_name,
            total_bytes_sent: proxy.total_bytes_sent,
            total_bytes_received: proxy.total_bytes_received,
            total_bytes: total,
        });
    }

    // 获取每日流量统计
    let mut daily = Vec::new();
    let start_date = Utc::now() - chrono::Duration::days(days);
    let start_date_str = start_date.format("%Y-%m-%d").to_string();

    let all_daily = TrafficDaily::find()
        .filter(traffic_daily::Column::Date.gte(&start_date_str))
        .all(db)
        .await?;

    let mut daily_map: HashMap<String, (i64, i64)> = HashMap::new();
    for d in all_daily {
        if !is_admin {
            if let Some(uid) = user_id {
                // 如果不是管理员，只显示有权限的代理的流量
                if !has_client_access(db, uid, d.client_id).await? {
                    continue;
                }
            }
        }
        let entry = daily_map.entry(d.date.clone()).or_insert((0, 0));
        entry.0 += d.bytes_sent;
        entry.1 += d.bytes_received;
    }

    for (date, (sent, received)) in daily_map {
        daily.push(DailyTraffic {
            date,
            total_bytes_sent: sent,
            total_bytes_received: received,
            total_bytes: sent + received,
        });
    }
    daily.sort_by(|a, b| a.date.cmp(&b.date));

    Ok(TrafficOverview {
        total_traffic: TotalTraffic {
            total_bytes_sent: total_sent,
            total_bytes_received: total_received,
            total_bytes: total_sent + total_received,
        },
        by_user: users,
        by_client: clients,
        by_proxy: proxies,
        daily_traffic: daily,
    })
}

/// 检查用户是否有访问客户端的权限（通过 client.user_id）
async fn has_client_access(db: &DatabaseConnection, user_id: i64, client_id: i64) -> Result<bool> {
    use crate::entity::client::Entity as Client;

    // 查找 client 并检查 user_id
    let client = match Client::find_by_id(client_id).one(db).await? {
        Some(c) => c,
        None => return Ok(false),
    };

    // 检查客户端是否属于该用户
    Ok(client.user_id == Some(user_id))
}
