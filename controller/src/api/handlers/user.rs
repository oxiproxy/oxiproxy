use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, PaginatorTrait, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use crate::{
    auth::{generate_random_password, hash_password},
    entity::{User, UserNode, Node},
    migration::get_connection,
    middleware::AuthUser,
};

use super::ApiResponse;

#[derive(Serialize)]
pub struct UserWithNodeCount {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
    pub created_at: String,
    pub updated_at: String,
    pub node_count: u64,
    #[serde(rename = "totalBytesSent")]
    pub total_bytes_sent: i64,
    #[serde(rename = "totalBytesReceived")]
    pub total_bytes_received: i64,
    #[serde(rename = "trafficQuotaGb")]
    pub traffic_quota_gb: Option<f64>,
    #[serde(rename = "remainingQuotaGb")]
    pub remaining_quota_gb: Option<f64>,
    #[serde(rename = "trafficResetCycle")]
    pub traffic_reset_cycle: String,
    #[serde(rename = "lastResetAt")]
    pub last_reset_at: Option<String>,
    #[serde(rename = "isTrafficExceeded")]
    pub is_traffic_exceeded: bool,
    #[serde(rename = "maxPortCount")]
    pub max_port_count: Option<i32>,
    #[serde(rename = "allowedPortRange")]
    pub allowed_port_range: Option<String>,
    #[serde(rename = "currentPortCount")]
    pub current_port_count: u64,
    #[serde(rename = "maxNodeCount")]
    pub max_node_count: Option<i32>,
    #[serde(rename = "maxClientCount")]
    pub max_client_count: Option<i32>,
    #[serde(rename = "currentClientCount")]
    pub current_client_count: u64,
}

#[derive(Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub password: Option<String>,
    pub is_admin: Option<bool>,
    pub traffic_quota_gb: Option<f64>,
    pub traffic_reset_cycle: Option<String>,
    pub max_port_count: Option<i32>,
    pub max_node_count: Option<i32>,
    pub max_client_count: Option<i32>,
}

#[derive(Deserialize)]
pub struct UpdateUserRequest {
    pub username: Option<String>,
    pub password: Option<String>,
    pub is_admin: Option<bool>,
    pub traffic_quota_gb: Option<f64>,
    pub traffic_reset_cycle: Option<String>,
    pub is_traffic_exceeded: Option<bool>,
    pub max_port_count: Option<i32>,
    pub allowed_port_range: Option<String>,
    pub max_node_count: Option<i32>,
    pub max_client_count: Option<i32>,
}

/// GET /api/users - Get all users (admin only)
pub async fn list_users(Extension(auth_user_opt): Extension<Option<AuthUser>>) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<UserWithNodeCount>>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<Vec<UserWithNodeCount>>::error("需要管理员权限".to_string()));
    }
    let db = get_connection().await;

    match User::find().all(db).await {
        Ok(users) => {
            // Count nodes for each user
            let mut users_with_count = Vec::new();
            for user in users {
                let node_count = match UserNode::find()
                    .filter(crate::entity::user_node::Column::UserId.eq(user.id))
                    .count(db)
                    .await
                {
                    Ok(count) => count,
                    Err(_) => 0,
                };

                // 获取最终配额（套餐配额 + 用户直接配额）
                let (final_traffic_quota_gb, final_max_port_count, final_max_node_count, final_max_client_count) = match crate::subscription_quota::get_user_final_quota(
                    user.id,
                    user.traffic_quota_gb,
                    user.max_port_count,
                    user.max_node_count,
                    user.max_client_count,
                    db,
                ).await {
                    Ok(quota) => quota,
                    Err(_) => (user.traffic_quota_gb, user.max_port_count, user.max_node_count, user.max_client_count),
                };

                // 使用最终配额计算剩余配额
                let remaining_quota_gb = if let Some(quota_gb) = final_traffic_quota_gb {
                    let total_used = user.total_bytes_sent + user.total_bytes_received;
                    let used_gb = crate::traffic_limiter::bytes_to_gb(total_used);
                    Some((quota_gb - used_gb).max(0.0))
                } else {
                    None
                };

                let current_port_count = crate::port_limiter::get_user_port_count(user.id, db).await.unwrap_or(0);

                let current_client_count = match crate::entity::Client::find()
                    .filter(crate::entity::client::Column::UserId.eq(user.id))
                    .count(db)
                    .await
                {
                    Ok(count) => count,
                    Err(_) => 0,
                };

                users_with_count.push(UserWithNodeCount {
                    id: user.id,
                    username: user.username.clone(),
                    is_admin: user.is_admin,
                    created_at: user.created_at.to_string(),
                    updated_at: user.updated_at.to_string(),
                    node_count,
                    total_bytes_sent: user.total_bytes_sent,
                    total_bytes_received: user.total_bytes_received,
                    traffic_quota_gb: final_traffic_quota_gb,
                    remaining_quota_gb,
                    traffic_reset_cycle: user.traffic_reset_cycle.clone(),
                    last_reset_at: user.last_reset_at.map(|d| d.to_string()),
                    is_traffic_exceeded: user.is_traffic_exceeded,
                    max_port_count: final_max_port_count,
                    allowed_port_range: user.allowed_port_range.clone(),
                    current_port_count,
                    max_node_count: final_max_node_count,
                    max_client_count: final_max_client_count,
                    current_client_count,
                });
            }

            (StatusCode::OK, ApiResponse::success(users_with_count))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<Vec<UserWithNodeCount>>::error(format!("Failed to list users: {}", e)),
        ),
    }
}

/// POST /api/users - Create a new user (admin only)
pub async fn create_user(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Json(req): Json<CreateUserRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("需要管理员权限".to_string()));
    }
    // Check if username already exists
    let db = get_connection().await;
    match User::find()
        .filter(crate::entity::user::Column::Username.eq(&req.username))
        .one(db)
        .await
    {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                ApiResponse::<serde_json::Value>::error("Username already exists".to_string()),
            )
        }
        Ok(None) => {}
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<serde_json::Value>::error(format!("Failed to check username: {}", e)),
            )
        }
    };

    // Hash password or generate random one
    let password = req.password.clone().unwrap_or_else(|| generate_random_password(16));
    let password_hash = match hash_password(&password) {
        Ok(hash) => hash,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<serde_json::Value>::error(format!("Failed to hash password: {}", e)),
            )
        }
    };

    // Create user
    let now = Utc::now().naive_utc();
    let new_user = crate::entity::user::ActiveModel {
        id: NotSet,
        username: Set(req.username),
        password_hash: Set(password_hash),
        is_admin: Set(req.is_admin.unwrap_or(false)),
        total_bytes_sent: Set(0),
        total_bytes_received: Set(0),
        traffic_quota_gb: Set(Some(req.traffic_quota_gb.unwrap_or(0.0))),
        traffic_reset_cycle: Set(req.traffic_reset_cycle.unwrap_or_else(|| "none".to_string())),
        last_reset_at: Set(None),
        is_traffic_exceeded: Set(false),
        max_port_count: Set(Some(req.max_port_count.unwrap_or(0))),
        allowed_port_range: Set(None),
        max_node_count: Set(Some(req.max_node_count.unwrap_or(0))),
        max_client_count: Set(Some(req.max_client_count.unwrap_or(0))),
        created_at: Set(now),
        updated_at: Set(now),
    };

    match new_user.insert(db).await {
        Ok(user) => {
            // Log generated password if random
            if req.password.is_none() {
                tracing::info!("Generated password for user '{}': {}", user.username, password);
            }

            // Return user without password hash
            let user_response = serde_json::json!({
                "id": user.id,
                "username": user.username,
                "is_admin": user.is_admin,
                "created_at": user.created_at,
                "updated_at": user.updated_at,
                "generated_password": if req.password.is_none() { Some(password) } else { None },
            });

            (StatusCode::OK, ApiResponse::<serde_json::Value>::success(user_response))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("Failed to create user: {}", e)),
        ),
    }
}

/// PUT /api/users/:id - Update a user (admin only)
pub async fn update_user(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("需要管理员权限".to_string()));
    }
    let db = get_connection().await;

    // Find user
    let user = match User::find_by_id(id).one(db).await {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                ApiResponse::<serde_json::Value>::error("User not found".to_string()),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<serde_json::Value>::error(format!("Failed to find user: {}", e)),
            )
        }
    };

    let mut user: crate::entity::user::ActiveModel = user.into();

    // Check if new username conflicts
    if let Some(new_username) = &req.username {
        match User::find()
            .filter(crate::entity::user::Column::Username.eq(new_username))
            .filter(crate::entity::user::Column::Id.ne(id))
            .one(db)
            .await
        {
            Ok(Some(_)) => {
                return (
                    StatusCode::CONFLICT,
                    ApiResponse::<serde_json::Value>::error(
                        "Username already exists".to_string(),
                    ),
                )
            }
            Ok(None) => {
                user.username = Set(new_username.clone());
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<serde_json::Value>::error(format!(
                        "Failed to check username: {}",
                        e
                    )),
                )
            }
        }
    }

    // Update password if provided
    if let Some(password) = &req.password {
        let password_hash = match hash_password(password) {
            Ok(hash) => hash,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<serde_json::Value>::error(format!(
                        "Failed to hash password: {}",
                        e
                    )),
                )
            }
        };
        user.password_hash = Set(password_hash);
    }

    // Update admin status if provided
    if let Some(is_admin) = req.is_admin {
        user.is_admin = Set(is_admin);
    }

    // Update traffic limits if provided
    if req.traffic_quota_gb.is_some() || req.traffic_quota_gb.is_none() {
        user.traffic_quota_gb = Set(req.traffic_quota_gb);
    }
    if let Some(cycle) = req.traffic_reset_cycle {
        user.traffic_reset_cycle = Set(cycle);
    }
    if let Some(exceeded) = req.is_traffic_exceeded {
        user.is_traffic_exceeded = Set(exceeded);
    }

    // Update port limits if provided
    if let Some(max_count) = req.max_port_count {
        user.max_port_count = Set(Some(max_count));
    }
    if let Some(range) = &req.allowed_port_range {
        // 验证端口范围格式
        if !range.is_empty() {
            match crate::port_limiter::parse_port_ranges(range) {
                Ok(_) => {
                    user.allowed_port_range = Set(Some(range.clone()));
                }
                Err(e) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        ApiResponse::<serde_json::Value>::error(format!(
                            "端口范围格式错误: {}",
                            e
                        )),
                    );
                }
            }
        } else {
            user.allowed_port_range = Set(None);
        }
    }

    // Update node/client limits if provided
    if let Some(max_count) = req.max_node_count {
        user.max_node_count = Set(Some(max_count));
    }
    if let Some(max_count) = req.max_client_count {
        user.max_client_count = Set(Some(max_count));
    }

    user.updated_at = Set(Utc::now().naive_utc());

    match user.update(db).await {
        Ok(updated) => {
            let user_response = serde_json::json!({
                "id": updated.id,
                "username": updated.username,
                "is_admin": updated.is_admin,
                "created_at": updated.created_at,
                "updated_at": updated.updated_at,
            });

            (StatusCode::OK, ApiResponse::<serde_json::Value>::success(user_response))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("Failed to update user: {}", e)),
        ),
    }
}

/// DELETE /api/users/:id - Delete a user (admin only)
pub async fn delete_user(Extension(auth_user_opt): Extension<Option<AuthUser>>, Path(id): Path<i64>) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<&str>::error("需要管理员权限".to_string()));
    }
    let db = get_connection().await;

    match User::delete_by_id(id).exec(db).await {
        Ok(_) => (StatusCode::OK, ApiResponse::success("User deleted successfully")),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<&str>::error(format!("Failed to delete user: {}", e)),
        ),
    }
}

/// GET /api/users/:id/nodes - Get user's node list (admin only)
pub async fn get_user_nodes(Extension(auth_user_opt): Extension<Option<AuthUser>>, Path(user_id): Path<i64>) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<crate::entity::node::Model>>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<Vec<crate::entity::node::Model>>::error("需要管理员权限".to_string()));
    }
    let db = get_connection().await;

    match UserNode::find()
        .filter(crate::entity::user_node::Column::UserId.eq(user_id))
        .find_also_related(crate::entity::Node)
        .all(db)
        .await
    {
        Ok(user_nodes) => {
            let nodes: Vec<_> = user_nodes
                .into_iter()
                .filter_map(|(_, node)| node)
                .collect();

            (StatusCode::OK, ApiResponse::success(nodes))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<Vec<crate::entity::node::Model>>::error(format!(
                "Failed to get user nodes: {}",
                e
            )),
        ),
    }
}

/// POST /api/users/:id/nodes/:node_id - Assign node to user (admin only)
pub async fn assign_node_to_user(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Path((user_id, node_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<&str>::error("需要管理员权限".to_string()));
    }
    let db = get_connection().await;

    // Check if user exists
    let user_model = match User::find_by_id(user_id).one(db).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                ApiResponse::<&str>::error("User not found".to_string()),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<&str>::error(format!("Failed to find user: {}", e)),
            )
        }
    };

    // Check if node exists and is dedicated
    let node = match Node::find_by_id(node_id).one(db).await {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                ApiResponse::<&str>::error("Node not found".to_string()),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<&str>::error(format!("Failed to find node: {}", e)),
            )
        }
    };

    // Only dedicated nodes can be assigned to users
    if node.node_type == "shared" {
        return (
            StatusCode::BAD_REQUEST,
            ApiResponse::<&str>::error("共享节点无需分配，所有用户均可使用".to_string()),
        );
    }

    // Check node count limit
    let (_, _, final_max_node_count, _) = match crate::subscription_quota::get_user_final_quota(
        user_id,
        user_model.traffic_quota_gb,
        user_model.max_port_count,
        user_model.max_node_count,
        user_model.max_client_count,
        db,
    ).await {
        Ok(q) => q,
        Err(_) => (user_model.traffic_quota_gb, user_model.max_port_count, user_model.max_node_count, user_model.max_client_count),
    };

    if let Some(max_count) = final_max_node_count {
        let current_count = match UserNode::find()
            .filter(crate::entity::user_node::Column::UserId.eq(user_id))
            .count(db)
            .await
        {
            Ok(c) => c,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<&str>::error(format!("Failed to count nodes: {}", e))),
        };
        if current_count >= max_count as u64 {
            return (
                StatusCode::BAD_REQUEST,
                ApiResponse::<&str>::error(format!("已达到最大节点数量限制: {}/{}", current_count, max_count)),
            );
        }
    }

    // Check if already assigned
    match UserNode::find()
        .filter(crate::entity::user_node::Column::UserId.eq(user_id))
        .filter(crate::entity::user_node::Column::NodeId.eq(node_id))
        .one(db)
        .await
    {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                ApiResponse::<&str>::error("Node already assigned to user".to_string()),
            )
        }
        Ok(None) => {}
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<&str>::error(format!("Failed to check assignment: {}", e)),
            )
        }
    };

    // Create assignment
    let now = Utc::now().naive_utc();
    let new_user_node = crate::entity::user_node::ActiveModel {
        id: NotSet,
        user_id: Set(user_id),
        node_id: Set(node_id),
        created_at: Set(now),
    };

    match new_user_node.insert(db).await {
        Ok(_) => (StatusCode::OK, ApiResponse::success("Node assigned successfully")),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<&str>::error(format!("Failed to assign node: {}", e)),
        ),
    }
}

/// DELETE /api/users/:id/nodes/:node_id - Remove node from user (admin only)
pub async fn remove_node_from_user(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Path((user_id, node_id)): Path<(i64, i64)>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };
    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<&str>::error("需要管理员权限".to_string()));
    }
    let db = get_connection().await;

    match UserNode::delete_many()
        .filter(crate::entity::user_node::Column::UserId.eq(user_id))
        .filter(crate::entity::user_node::Column::NodeId.eq(node_id))
        .exec(db)
        .await
    {
        Ok(result) => {
            if result.rows_affected > 0 {
                (
                    StatusCode::OK,
                    ApiResponse::success("Node removed successfully"),
                )
            } else {
                (
                    StatusCode::NOT_FOUND,
                    ApiResponse::<&str>::error("Assignment not found".to_string()),
                )
            }
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<&str>::error(format!("Failed to remove node: {}", e)),
        ),
    }
}

/// POST /api/users/:id/adjust-quota - Adjust user quota (admin only)
#[derive(Deserialize)]
pub struct AdjustQuotaRequest {
    pub quota_change_gb: f64,
}

pub async fn adjust_user_quota(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Path(user_id): Path<i64>,
    Json(req): Json<AdjustQuotaRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<String>::error("未认证".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<String>::error("只有管理员可以调整用户配额".to_string()));
    }

    let db = get_connection().await;

    // 查找用户
    let user = match User::find_by_id(user_id).one(db).await {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<String>::error("用户不存在".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("查询用户失败: {}", e))),
    };

    // 计算新配额
    let current_quota = user.traffic_quota_gb.unwrap_or(0.0);
    let new_quota = current_quota + req.quota_change_gb;

    if new_quota < 0.0 {
        return (StatusCode::BAD_REQUEST, ApiResponse::<String>::error("配额不能为负数".to_string()));
    }

    // 如果是减少配额，需要检查是否会影响已分配的客户端配额
    if req.quota_change_gb < 0.0 {
        use crate::entity::{user_client, UserClient, Client};

        // 计算用户已使用的流量
        let user_used_gb = crate::traffic_limiter::bytes_to_gb(user.total_bytes_sent + user.total_bytes_received);

        // 查询用户所有客户端已分配的配额总和
        let user_clients = match UserClient::find()
            .filter(user_client::Column::UserId.eq(user_id))
            .all(db)
            .await
        {
            Ok(uc) => uc,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("查询客户端失败: {}", e))),
        };

        let mut total_allocated_gb = 0.0;
        for uc in user_clients {
            if let Ok(Some(client)) = Client::find_by_id(uc.client_id).one(db).await {
                if let Some(quota) = client.traffic_quota_gb {
                    total_allocated_gb += quota;
                }
            }
        }

        // 检查新配额是否足够覆盖已使用和已分配的配额
        if new_quota < user_used_gb + total_allocated_gb {
            let reason = format!(
                "配额不足: 新配额 {:.2} GB 小于已使用 {:.2} GB + 已分配 {:.2} GB = {:.2} GB",
                new_quota,
                user_used_gb,
                total_allocated_gb,
                user_used_gb + total_allocated_gb
            );
            return (StatusCode::BAD_REQUEST, ApiResponse::<String>::error(reason));
        }
    }

    // 更新用户配额
    let mut user_active: crate::entity::user::ActiveModel = user.into();
    user_active.traffic_quota_gb = Set(Some(new_quota));
    user_active.updated_at = Set(Utc::now().naive_utc());

    match user_active.update(db).await {
        Ok(_) => {
            let message = if req.quota_change_gb > 0.0 {
                format!("配额增加成功: +{:.2} GB，当前配额: {:.2} GB", req.quota_change_gb, new_quota)
            } else {
                format!("配额减少成功: {:.2} GB，当前配额: {:.2} GB", req.quota_change_gb, new_quota)
            };
            (StatusCode::OK, ApiResponse::success(message))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("更新配额失败: {}", e))),
    }
}

/// GET /api/users/:id/quota-info - Get user quota information
#[derive(Serialize)]
pub struct UserQuotaInfo {
    pub user_id: i64,
    pub username: String,
    pub total_quota_gb: Option<f64>,
    pub used_gb: f64,
    pub allocated_to_clients_gb: f64,
    pub available_gb: f64,
    pub quota_usage_percent: Option<f64>,
}

pub async fn get_user_quota_info(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Path(user_id): Path<i64>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<UserQuotaInfo>::error("未认证".to_string())),
    };

    // 非管理员只能查看自己的配额信息
    if !auth_user.is_admin && auth_user.id != user_id {
        return (StatusCode::FORBIDDEN, ApiResponse::<UserQuotaInfo>::error("无权限查看此用户配额".to_string()));
    }

    let db = get_connection().await;

    let user = match User::find_by_id(user_id).one(db).await {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<UserQuotaInfo>::error("用户不存在".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<UserQuotaInfo>::error(format!("查询失败: {}", e))),
    };

    let used_gb = crate::traffic_limiter::bytes_to_gb(user.total_bytes_sent + user.total_bytes_received);

    // 计算已分配给客户端的配额
    use crate::entity::{user_client, UserClient, Client};

    let user_clients = match UserClient::find()
        .filter(user_client::Column::UserId.eq(user_id))
        .all(db)
        .await
    {
        Ok(uc) => uc,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<UserQuotaInfo>::error(format!("查询客户端失败: {}", e))),
    };

    let mut allocated_to_clients_gb = 0.0;
    for uc in user_clients {
        if let Ok(Some(client)) = Client::find_by_id(uc.client_id).one(db).await {
            if let Some(quota) = client.traffic_quota_gb {
                allocated_to_clients_gb += quota;
            }
        }
    }

    // 获取最终配额（套餐配额 + 用户直接配额）
    let (final_traffic_quota_gb, _, _, _) = match crate::subscription_quota::get_user_final_quota(
        user_id,
        user.traffic_quota_gb,
        user.max_port_count,
        user.max_node_count,
        user.max_client_count,
        db,
    ).await {
        Ok(quota) => quota,
        Err(_) => (user.traffic_quota_gb, user.max_port_count, user.max_node_count, user.max_client_count),
    };

    let total_quota_gb = final_traffic_quota_gb;
    let available_gb = if let Some(quota) = total_quota_gb {
        (quota - used_gb - allocated_to_clients_gb).max(0.0)
    } else {
        f64::INFINITY
    };

    let quota_usage_percent = if let Some(quota) = total_quota_gb {
        Some(((used_gb + allocated_to_clients_gb) / quota * 100.0).min(100.0))
    } else {
        None
    };

    let info = UserQuotaInfo {
        user_id: user.id,
        username: user.username,
        total_quota_gb,
        used_gb,
        allocated_to_clients_gb,
        available_gb,
        quota_usage_percent,
    };

    (StatusCode::OK, ApiResponse::success(info))
}
