use axum::{
    extract::Extension,
    http::StatusCode,
    response::{IntoResponse, Json},
};

use crate::{
    auth::{hash_password, verify_password},
    entity::User,
    jwt::generate_token,
    middleware::AuthUser,
    migration::get_connection,
    AppState,
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, QueryFilter, Set};
use serde::{Deserialize, Serialize};

use super::ApiResponse;

#[derive(Serialize)]
pub struct LoginResponse {
    pub token: String,
    pub user: UserInfo,
}

#[derive(Serialize)]
pub struct UserInfo {
    pub id: i64,
    pub username: String,
    pub is_admin: bool,
}

#[derive(Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

/// POST /api/auth/login - User login
pub async fn login(
    Extension(app_state): Extension<AppState>,
    Json(req): Json<LoginRequest>,
) -> impl IntoResponse {
    let db = get_connection().await;

    // Find user by username
    let user = match User::find()
        .filter(crate::entity::user::Column::Username.eq(&req.username))
        .one(db)
        .await
    {
        Ok(Some(user)) => user,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                ApiResponse::<LoginResponse>::error(
                    "Invalid username or password".to_string(),
                ),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("Login failed: {}", e)),
            )
        }
    };

    // Verify password
    match verify_password(&req.password, &user.password_hash) {
        Ok(true) => {}
        Ok(false) => {
            return (
                StatusCode::UNAUTHORIZED,
                ApiResponse::<LoginResponse>::error(
                    "Invalid username or password".to_string(),
                ),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("Login failed: {}", e)),
            )
        }
    };

    // Get JWT secret from config
    let jwt_secret = match app_state.config.get_jwt_secret() {
        Ok(secret) => secret,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("JWT configuration error: {}", e)),
            )
        }
    };

    // Generate JWT token
    let token = match generate_token(
        user.id,
        &user.username,
        user.is_admin,
        &jwt_secret,
        app_state.config.jwt_expiration_hours,
    ) {
        Ok(token) => token,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("Failed to generate token: {}", e)),
            )
        }
    };

    let response = LoginResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            is_admin: user.is_admin,
        },
    };

    (StatusCode::OK, ApiResponse::success(response))
}

/// GET /api/auth/me - Get current user info
pub async fn me(Extension(auth_user): Extension<Option<AuthUser>>) -> impl IntoResponse {
    let auth_user = match auth_user {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<UserInfo>::error("Not authenticated".to_string())),
    };
    let user_info = UserInfo {
        id: auth_user.id,
        username: auth_user.username,
        is_admin: auth_user.is_admin,
    };

    (StatusCode::OK, ApiResponse::success(user_info))
}

#[derive(Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct RegisterStatusResponse {
    pub enabled: bool,
}

/// GET /api/auth/register-status - Check if registration is enabled
pub async fn get_register_status(
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let enabled = app_state.config_manager.get_bool("enable_registration", false).await;
    (StatusCode::OK, ApiResponse::success(RegisterStatusResponse { enabled }))
}

/// POST /api/auth/register - User registration
pub async fn register(
    Extension(app_state): Extension<AppState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    // 检查是否允许注册
    let enabled = app_state.config_manager.get_bool("enable_registration", false).await;
    if !enabled {
        return (
            StatusCode::FORBIDDEN,
            ApiResponse::<LoginResponse>::error("注册功能未开放".to_string()),
        );
    }

    // 校验用户名
    let username = req.username.trim().to_string();
    if username.len() < 3 || username.len() > 20 {
        return (
            StatusCode::BAD_REQUEST,
            ApiResponse::<LoginResponse>::error("用户名长度需要 3-20 个字符".to_string()),
        );
    }

    // 校验密码
    if req.password.len() < 6 {
        return (
            StatusCode::BAD_REQUEST,
            ApiResponse::<LoginResponse>::error("密码长度不能少于 6 个字符".to_string()),
        );
    }

    let db = get_connection().await;

    // 检查用户名是否已存在
    match User::find()
        .filter(crate::entity::user::Column::Username.eq(&username))
        .one(db)
        .await
    {
        Ok(Some(_)) => {
            return (
                StatusCode::CONFLICT,
                ApiResponse::<LoginResponse>::error("用户名已存在".to_string()),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("数据库错误: {}", e)),
            );
        }
        Ok(None) => {}
    }

    // 哈希密码
    let password_hash = match hash_password(&req.password) {
        Ok(hash) => hash,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("密码加密失败: {}", e)),
            );
        }
    };

    // 创建用户
    let now = Utc::now().naive_utc();
    let cm = &app_state.config_manager;
    let default_quota = cm.get_float("default_traffic_quota_gb", 0.0).await;
    let default_max_port = cm.get_number("default_max_port_count", 0).await as i32;
    let default_max_node = cm.get_number("default_max_node_count", 0).await as i32;
    let default_max_client = cm.get_number("default_max_client_count", 0).await as i32;
    let new_user = crate::entity::user::ActiveModel {
        id: NotSet,
        username: Set(username.clone()),
        password_hash: Set(password_hash),
        is_admin: Set(false),
        total_bytes_sent: Set(0),
        total_bytes_received: Set(0),
        traffic_reset_cycle: Set("none".to_string()),
        last_reset_at: Set(None),
        is_traffic_exceeded: Set(false),
        traffic_quota_gb: Set(Some(default_quota)),
        max_port_count: Set(Some(default_max_port)),
        allowed_port_range: Set(None),
        max_node_count: Set(Some(default_max_node)),
        max_client_count: Set(Some(default_max_client)),
        created_at: Set(now),
        updated_at: Set(now),
    };

    let user = match new_user.insert(db).await {
        Ok(user) => user,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("创建用户失败: {}", e)),
            );
        }
    };

    // 生成 JWT token（注册后自动登录）
    let jwt_secret = match app_state.config.get_jwt_secret() {
        Ok(secret) => secret,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("JWT 配置错误: {}", e)),
            );
        }
    };

    let token = match generate_token(
        user.id,
        &user.username,
        user.is_admin,
        &jwt_secret,
        app_state.config.jwt_expiration_hours,
    ) {
        Ok(token) => token,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<LoginResponse>::error(format!("生成令牌失败: {}", e)),
            );
        }
    };

    let response = LoginResponse {
        token,
        user: UserInfo {
            id: user.id,
            username: user.username,
            is_admin: user.is_admin,
        },
    };

    (StatusCode::OK, ApiResponse::success(response))
}
