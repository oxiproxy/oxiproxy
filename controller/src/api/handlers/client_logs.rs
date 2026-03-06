use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
};
use tracing::{error, info};

use crate::{middleware::AuthUser, AppState};
use common::protocol::control::LogEntry;

use super::ApiResponse;

/// GET /api/clients/{id}/logs - 获取客户端日志
pub async fn get_client_logs(
    Path(client_id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<LogEntry>>::error("Not authenticated".to_string())),
    };

    // 校验客户端归属权
    let db = crate::migration::get_connection().await;
    if let Err((status, msg)) = super::verify_client_ownership(&auth_user, client_id, db).await {
        return (status, ApiResponse::<Vec<LogEntry>>::error(msg));
    }

    info!("请求客户端 {} 的日志", client_id);

    // 直接通过 ClientStreamManager 向客户端请求日志
    match app_state.client_stream_manager.fetch_client_logs(client_id, 200).await {
        Ok(logs) => {
            info!("成功获取客户端 {} 的 {} 条日志", client_id, logs.len());
            (StatusCode::OK, ApiResponse::success(logs))
        }
        Err(e) => {
            error!("获取客户端日志失败: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<Vec<LogEntry>>::error(format!(
                    "获取日志失败: {}",
                    e
                )),
            )
        }
    }
}
