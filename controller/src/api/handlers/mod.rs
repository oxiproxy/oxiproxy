pub mod auth;
pub mod client;
pub mod proxy;
pub mod user;
pub mod traffic;
pub mod dashboard;
pub mod client_logs;
pub mod system_config;
pub mod node;
pub mod client_config;
pub mod subscription;
pub mod user_subscription;
pub mod version;

// Re-export common handler modules
pub use auth::*;
pub use client::*;
pub use proxy::*;
pub use user::*;
pub use traffic::*;
pub use dashboard::*;
pub use client_logs::*;
pub use system_config::*;
pub use node::*;
pub use client_config::*;
pub use subscription::*;
pub use user_subscription::*;
pub use version::*;

use serde::Serialize;
use sea_orm::{DatabaseConnection, EntityTrait, ColumnTrait, QueryFilter};
use crate::middleware::AuthUser;
use crate::entity::{Client, Proxy};

#[derive(Serialize)]
pub struct ApiResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub message: String,
}

impl<T> ApiResponse<T> {
    pub fn success(data: T) -> axum::response::Json<Self> {
        axum::response::Json(Self {
            success: true,
            data: Some(data),
            message: "Success".to_string(),
        })
    }

    pub fn error(message: String) -> axum::response::Json<Self> {
        axum::response::Json(Self {
            success: false,
            data: None,
            message,
        })
    }
}

/// 校验当前用户是否拥有指定 client（admin 跳过检查）
/// 返回 Ok(client) 或 Err(错误信息)
pub async fn verify_client_ownership(
    auth_user: &AuthUser,
    client_id: i64,
    db: &DatabaseConnection,
) -> Result<crate::entity::client::Model, (axum::http::StatusCode, String)> {
    let client = Client::find_by_id(client_id)
        .one(db)
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("数据库查询失败: {}", e)))?
        .ok_or((axum::http::StatusCode::NOT_FOUND, "客户端不存在".to_string()))?;

    if !auth_user.is_admin && client.user_id != Some(auth_user.id) {
        return Err((axum::http::StatusCode::FORBIDDEN, "无权访问此客户端".to_string()));
    }

    Ok(client)
}

/// 校验当前用户是否拥有指定 proxy（通过 proxy→client→user 链路，admin 跳过）
pub async fn verify_proxy_ownership(
    auth_user: &AuthUser,
    proxy: &crate::entity::proxy::Model,
    db: &DatabaseConnection,
) -> Result<(), (axum::http::StatusCode, String)> {
    if auth_user.is_admin {
        return Ok(());
    }

    let client_id: i64 = proxy.client_id.parse()
        .map_err(|_| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "无效的客户端 ID".to_string()))?;

    let client = Client::find_by_id(client_id)
        .one(db)
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("数据库查询失败: {}", e)))?
        .ok_or((axum::http::StatusCode::NOT_FOUND, "关联客户端不存在".to_string()))?;

    if client.user_id != Some(auth_user.id) {
        return Err((axum::http::StatusCode::FORBIDDEN, "无权访问此代理".to_string()));
    }

    Ok(())
}

/// 校验当前用户是否拥有指定 proxy group 中的代理
pub async fn verify_proxy_group_ownership(
    auth_user: &AuthUser,
    group_id: &str,
    db: &DatabaseConnection,
) -> Result<Vec<crate::entity::proxy::Model>, (axum::http::StatusCode, String)> {
    let proxies = Proxy::find()
        .filter(crate::entity::proxy::Column::GroupId.eq(group_id))
        .all(db)
        .await
        .map_err(|e| (axum::http::StatusCode::INTERNAL_SERVER_ERROR, format!("查询代理组失败: {}", e)))?;

    if proxies.is_empty() {
        return Err((axum::http::StatusCode::NOT_FOUND, "代理组不存在".to_string()));
    }

    if !auth_user.is_admin {
        // 校验第一条 proxy 的 client 归属即可（同组 proxy 属于同一 client）
        verify_proxy_ownership(auth_user, &proxies[0], db).await?;
    }

    Ok(proxies)
}
