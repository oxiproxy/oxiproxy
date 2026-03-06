use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, PaginatorTrait, QueryFilter, Set};
use serde::Deserialize;
use uuid::Uuid;

use crate::{entity::Client, migration::get_connection, middleware::AuthUser};

use super::ApiResponse;

#[derive(Deserialize)]
pub struct CreateClientRequest {
    pub name: String,
    pub token: Option<String>,
    pub region: Option<String>,
    pub traffic_reset_cycle: Option<String>,
    pub traffic_quota_gb: Option<f64>,
}

pub async fn list_clients(Extension(auth_user_opt): Extension<Option<AuthUser>>) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<crate::entity::client::Model>>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    let clients = if auth_user.is_admin {
        // Admin can see all clients
        match Client::find().all(db).await {
            Ok(clients) => clients,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<Vec<crate::entity::client::Model>>::error(format!(
                        "Failed to list clients: {}",
                        e
                    )),
                )
            }
        }
    } else {
        // Regular users can only see their own clients (based on client.user_id)
        match Client::find()
            .filter(crate::entity::client::Column::UserId.eq(auth_user.id))
            .all(db)
            .await
        {
            Ok(clients) => clients,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<Vec<crate::entity::client::Model>>::error(format!(
                        "Failed to list clients: {}",
                        e
                    )),
                )
            }
        }
    };

    (StatusCode::OK, ApiResponse::success(clients))
}

pub async fn create_client(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Json(req): Json<CreateClientRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<crate::entity::client::Model>::error("未认证".to_string())),
    };

    let db = get_connection().await;

    // 检查客户端数量限制
    if let Ok(Some(user_model)) = crate::entity::User::find_by_id(auth_user.id).one(db).await {
        let (_, _, _, final_max_client_count) = match crate::subscription_quota::get_user_final_quota(
            auth_user.id,
            user_model.traffic_quota_gb,
            user_model.max_port_count,
            user_model.max_node_count,
            user_model.max_client_count,
            db,
        ).await {
            Ok(q) => q,
            Err(_) => (user_model.traffic_quota_gb, user_model.max_port_count, user_model.max_node_count, user_model.max_client_count),
        };

        if let Some(max_count) = final_max_client_count {
            let current_count = match Client::find()
                .filter(crate::entity::client::Column::UserId.eq(auth_user.id))
                .count(db)
                .await
            {
                Ok(c) => c,
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<crate::entity::client::Model>::error(format!("查询客户端数量失败: {}", e))),
            };
            if current_count >= max_count as u64 {
                return (
                    StatusCode::BAD_REQUEST,
                    ApiResponse::<crate::entity::client::Model>::error(format!("已达到最大客户端数量限制: {}/{}", current_count, max_count)),
                );
            }
        }
    }

    let token = req.token.unwrap_or_else(|| Uuid::new_v4().to_string());
    let now = Utc::now().naive_utc();
    let new_client = crate::entity::client::ActiveModel {
        id: NotSet,
        name: Set(req.name),
        token: Set(token.clone()),
        is_online: NotSet,
        public_ip: Set(None),
        region: Set(req.region),
        user_id: Set(Some(auth_user.id)),
        version: Set(None),
        total_bytes_sent: Set(0),
        total_bytes_received: Set(0),
        traffic_quota_gb: Set(req.traffic_quota_gb),
        traffic_reset_cycle: Set(req.traffic_reset_cycle.unwrap_or_else(|| "none".to_string())),
        last_reset_at: Set(None),
        is_traffic_exceeded: Set(false),
        created_at: Set(now),
        updated_at: Set(now),
    };
    match new_client.insert(db).await {
        Ok(client) => (StatusCode::OK, ApiResponse::success(client)),
        Err(e) => {
            eprintln!("Failed to create client: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<crate::entity::client::Model>::error(format!(
                    "Failed to create client: {}",
                    e
                )),
            )
        }
    }
}

pub async fn get_client(Path(id): Path<i64>, Extension(auth_user_opt): Extension<Option<AuthUser>>) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<crate::entity::client::Model>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;
    match super::verify_client_ownership(&auth_user, id, db).await {
        Ok(client) => (StatusCode::OK, ApiResponse::success(client)),
        Err((status, msg)) => (status, ApiResponse::<crate::entity::client::Model>::error(msg)),
    }
}

pub async fn delete_client(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    // 校验归属权
    if let Err((status, msg)) = super::verify_client_ownership(&auth_user, id, db).await {
        return (status, ApiResponse::<&str>::error(msg));
    }

    match Client::delete_by_id(id).exec(db).await {
        Ok(_) => (StatusCode::OK, ApiResponse::success("Client deleted successfully")),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<&str>::error(format!("Failed to delete client: {}", e)),
        ),
    }
}

#[derive(Deserialize)]
pub struct UpdateClientRequest {
    pub name: Option<String>,
    pub region: Option<String>,
    pub traffic_quota_gb: Option<f64>,
    pub traffic_reset_cycle: Option<String>,
    pub is_traffic_exceeded: Option<bool>,
}

pub async fn update_client(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Json(req): Json<UpdateClientRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<crate::entity::client::Model>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<crate::entity::client::Model>::error("Only admin can update client".to_string()));
    }

    let db = get_connection().await;

    let client = match Client::find_by_id(id).one(db).await {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<crate::entity::client::Model>::error("Client not found".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<crate::entity::client::Model>::error(format!("Failed to find client: {}", e))),
    };

    let mut client_active: crate::entity::client::ActiveModel = client.into();

    if let Some(name) = req.name {
        client_active.name = Set(name);
    }
    if let Some(region) = req.region {
        client_active.region = Set(Some(region));
    }
    if req.traffic_quota_gb.is_some() || req.traffic_quota_gb.is_none() {
        client_active.traffic_quota_gb = Set(req.traffic_quota_gb);
    }
    if let Some(cycle) = req.traffic_reset_cycle {
        client_active.traffic_reset_cycle = Set(cycle);
    }
    if let Some(exceeded) = req.is_traffic_exceeded {
        client_active.is_traffic_exceeded = Set(exceeded);
    }

    client_active.updated_at = Set(Utc::now().naive_utc());

    match client_active.update(db).await {
        Ok(updated) => (StatusCode::OK, ApiResponse::success(updated)),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<crate::entity::client::Model>::error(format!("Failed to update client: {}", e))),
    }
}

/// 为客户端分配流量配额
#[derive(Deserialize)]
pub struct AllocateQuotaRequest {
    pub quota_gb: f64,
}

pub async fn allocate_client_quota(
    Path(client_id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Json(req): Json<AllocateQuotaRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<String>::error("未认证".to_string())),
    };

    if req.quota_gb < 0.0 {
        return (StatusCode::BAD_REQUEST, ApiResponse::<String>::error("配额不能为负数".to_string()));
    }

    let db = get_connection().await;

    // 检查客户端是否存在
    let client = match Client::find_by_id(client_id).one(db).await {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<String>::error("客户端不存在".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("查询客户端失败: {}", e))),
    };

    // 获取客户端当前配额
    let current_quota = client.traffic_quota_gb.unwrap_or(0.0);
    let quota_diff = req.quota_gb - current_quota;

    // 如果不是管理员，需要检查用户配额
    if !auth_user.is_admin {
        // 检查用户是否有权限访问此客户端
        let has_access = match crate::entity::UserClient::find()
            .filter(crate::entity::user_client::Column::UserId.eq(auth_user.id))
            .filter(crate::entity::user_client::Column::ClientId.eq(client_id))
            .one(db)
            .await
        {
            Ok(Some(_)) => true,
            Ok(None) => false,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("检查权限失败: {}", e))),
        };

        if !has_access {
            return (StatusCode::FORBIDDEN, ApiResponse::<String>::error("无权限访问此客户端".to_string()));
        }

        // 检查用户配额是否足够（仅在增加配额时检查）
        if quota_diff > 0.0 {
            match crate::traffic_limiter::check_user_quota_allocation(auth_user.id, quota_diff, db).await {
                Ok((true, _)) => {},
                Ok((false, reason)) => return (StatusCode::BAD_REQUEST, ApiResponse::<String>::error(reason)),
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("检查配额失败: {}", e))),
            }
        }
    }

    // 更新客户端配额
    let mut client_active: crate::entity::client::ActiveModel = client.into();
    client_active.traffic_quota_gb = Set(Some(req.quota_gb));
    client_active.updated_at = Set(Utc::now().naive_utc());

    match client_active.update(db).await {
        Ok(_) => (StatusCode::OK, ApiResponse::success(format!("配额分配成功: {:.2} GB", req.quota_gb))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<String>::error(format!("更新配额失败: {}", e))),
    }
}

/// 获取客户端流量详情（包含剩余配额）
use serde::Serialize;

#[derive(Serialize)]
pub struct ClientTrafficInfo {
    pub client_id: i64,
    pub client_name: String,
    pub total_bytes_sent: i64,
    pub total_bytes_received: i64,
    pub total_bytes: i64,
    pub quota_gb: Option<f64>,
    pub remaining_quota_gb: Option<f64>,
    pub quota_usage_percent: Option<f64>,
    pub is_traffic_exceeded: bool,
}

pub async fn get_client_traffic(
    Path(client_id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<ClientTrafficInfo>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    let client = match super::verify_client_ownership(&auth_user, client_id, db).await {
        Ok(c) => c,
        Err((status, msg)) => return (status, ApiResponse::<ClientTrafficInfo>::error(msg)),
    };

    let remaining_quota_gb = crate::traffic_limiter::calculate_client_remaining_quota(&client);
    let quota_usage_percent = if let (Some(quota), Some(remaining)) = (client.traffic_quota_gb, remaining_quota_gb) {
        Some(((quota - remaining) / quota * 100.0).min(100.0))
    } else {
        None
    };

    let info = ClientTrafficInfo {
        client_id: client.id,
        client_name: client.name,
        total_bytes_sent: client.total_bytes_sent,
        total_bytes_received: client.total_bytes_received,
        total_bytes: client.total_bytes_sent + client.total_bytes_received,
        quota_gb: client.traffic_quota_gb,
        remaining_quota_gb,
        quota_usage_percent,
        is_traffic_exceeded: client.is_traffic_exceeded,
    };

    (StatusCode::OK, ApiResponse::success(info))
}
