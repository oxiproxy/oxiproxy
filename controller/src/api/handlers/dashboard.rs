use sea_orm::{DatabaseConnection, DbErr, EntityTrait, ColumnTrait, QueryFilter};
use crate::entity;
use crate::middleware::AuthUser;
use super::ApiResponse;
use crate::migration::get_connection;

#[derive(Debug, serde::Serialize)]
pub struct DashboardStats {
    pub total_clients: i64,
    pub total_proxies: i64,
    pub online_clients: i64,
    pub enabled_proxies: i64,
    pub total_nodes: i64,
    pub online_nodes: i64,
    pub user_total_quota_gb: Option<f64>,
    pub user_traffic: UserTrafficStats,
}

#[derive(Debug, serde::Serialize)]
pub struct UserTrafficStats {
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
}

/// 获取指定用户的仪表板统计数据
pub async fn get_user_dashboard_stats(
    axum::extract::Path(user_id): axum::extract::Path<i64>,
    axum::extract::Extension(auth_user_opt): axum::extract::Extension<Option<AuthUser>>,
) -> axum::response::Json<ApiResponse<DashboardStats>> {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return ApiResponse::error("Not authenticated".to_string()),
    };

    // 非管理员只能查看自己的统计
    if !auth_user.is_admin && auth_user.id != user_id {
        return ApiResponse::error("无权访问此用户的统计数据".to_string());
    }

    let db = get_connection().await;

    // 查询用户信息
    let user = match entity::User::find_by_id(user_id).one(db).await {
        Ok(Some(u)) => u,
        Ok(None) => return ApiResponse::error("User not found".to_string()),
        Err(e) => return ApiResponse::error(format!("Database error: {}", e)),
    };

    let is_admin = user.is_admin;

    // 统计客户端数量
    let (total_clients, online_clients) = if is_admin {
        // 管理员可以看到所有客户端
        match entity::Client::find().all(db).await {
            Ok(clients) => {
                let online_count = clients.iter().filter(|c| c.is_online).count() as i64;
                (clients.len() as i64, online_count)
            }
            Err(_) => (0, 0),
        }
    } else {
        // 普通用户只统计绑定的客户端
        match get_user_clients(db, user_id).await {
            Ok(clients) => {
                let online_count = clients.iter().filter(|c| c.is_online).count() as i64;
                (clients.len() as i64, online_count)
            }
            Err(_) => (0, 0),
        }
    };

    // 统计代理数量
    let (total_proxies, enabled_proxies) = if is_admin {
        // 管理员可以看到所有代理
        match entity::Proxy::find().all(db).await {
            Ok(proxies) => {
                let enabled_count = proxies.iter().filter(|p| p.enabled).count() as i64;
                (proxies.len() as i64, enabled_count)
            }
            Err(_) => (0, 0),
        }
    } else {
        // 普通用户只统计绑定客户端的代理
        match get_user_proxies(db, user_id).await {
            Ok(proxies) => {
                let enabled_count = proxies.iter().filter(|p| p.enabled).count() as i64;
                (proxies.len() as i64, enabled_count)
            }
            Err(_) => (0, 0),
        }
    };

    // 获取用户流量统计（从代理表汇总）
    let user_traffic = if is_admin {
        // 管理员：汇总所有代理的流量
        match entity::Proxy::find().all(db).await {
            Ok(proxies) => {
                let total_sent: i64 = proxies.iter().map(|p| p.total_bytes_sent).sum();
                let total_received: i64 = proxies.iter().map(|p| p.total_bytes_received).sum();
                UserTrafficStats {
                    total_bytes_sent: total_sent,
                    total_bytes_received: total_received,
                    total_bytes: total_sent + total_received,
                }
            }
            Err(_) => UserTrafficStats {
                total_bytes_sent: 0,
                total_bytes_received: 0,
                total_bytes: 0,
            },
        }
    } else {
        // 普通用户：汇总绑定客户端的所有代理流量
        match get_user_proxies(db, user_id).await {
            Ok(proxies) => {
                let total_sent: i64 = proxies.iter().map(|p| p.total_bytes_sent).sum();
                let total_received: i64 = proxies.iter().map(|p| p.total_bytes_received).sum();
                UserTrafficStats {
                    total_bytes_sent: total_sent,
                    total_bytes_received: total_received,
                    total_bytes: total_sent + total_received,
                }
            }
            Err(_) => UserTrafficStats {
                total_bytes_sent: 0,
                total_bytes_received: 0,
                total_bytes: 0,
            },
        }
    };

    // 统计节点数量
    let (total_nodes, online_nodes) = match entity::Node::find().all(db).await {
        Ok(nodes) => {
            let online_count = nodes.iter().filter(|n| n.is_online).count() as i64;
            (nodes.len() as i64, online_count)
        }
        Err(_) => (0, 0),
    };

    let stats = DashboardStats {
        total_clients,
        total_proxies,
        online_clients,
        enabled_proxies,
        total_nodes,
        online_nodes,
        user_total_quota_gb: user.traffic_quota_gb,
        user_traffic,
    };

    ApiResponse::success(stats)
}

/// 获取用户绑定的客户端（通过 client.user_id）
async fn get_user_clients(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<entity::client::Model>, DbErr> {
    // 直接查询用户的客户端
    let clients = entity::Client::find()
        .filter(entity::client::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    Ok(clients)
}

/// 获取用户可访问的代理（通过绑定的客户端）
async fn get_user_proxies(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<entity::proxy::Model>, DbErr> {
    let clients = get_user_clients(db, user_id).await?;

    let mut all_proxies = Vec::new();
    for client in clients {
        let client_id = client.id.to_string();
        let proxies = entity::Proxy::find()
            .filter(entity::proxy::Column::ClientId.eq(&client_id))
            .all(db)
            .await?;

        all_proxies.extend(proxies);
    }

    Ok(all_proxies)
}
