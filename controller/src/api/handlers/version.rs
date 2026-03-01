use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
};
use sea_orm::EntityTrait;
use tokio::sync::RwLock;
use chrono::{Utc, NaiveDateTime};

use crate::middleware::AuthUser;
use crate::AppState;
use crate::migration::get_connection;
use super::ApiResponse;

#[derive(Clone, serde::Serialize)]
struct CachedRelease {
    version: String,
    published_at: String,
    html_url: String,
    fetched_at: NaiveDateTime,
}

static RELEASE_CACHE: std::sync::OnceLock<RwLock<Option<CachedRelease>>> = std::sync::OnceLock::new();

fn get_cache() -> &'static RwLock<Option<CachedRelease>> {
    RELEASE_CACHE.get_or_init(|| RwLock::new(None))
}

const CACHE_TTL_SECS: i64 = 300;

#[derive(serde::Deserialize)]
struct GitHubRelease {
    tag_name: String,
    published_at: Option<String>,
    html_url: String,
}

async fn fetch_latest_release() -> anyhow::Result<CachedRelease> {
    let client = reqwest::Client::builder()
        .user_agent("OxiProxy-Controller")
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp: GitHubRelease = client
        .get("https://api.github.com/repos/oxiproxy/oxiproxy/releases/latest")
        .send()
        .await?
        .json()
        .await?;

    let version = resp.tag_name.trim_start_matches('v').to_string();

    Ok(CachedRelease {
        version,
        published_at: resp.published_at.unwrap_or_default(),
        html_url: resp.html_url,
        fetched_at: Utc::now().naive_utc(),
    })
}

/// GET /api/system/latest-version
pub async fn get_latest_version(
    Extension(auth_user): Extension<Option<AuthUser>>,
) -> impl IntoResponse {
    let auth_user = match auth_user {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("未认证".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("仅管理员".to_string()));
    }

    let cache = get_cache();

    {
        let cached = cache.read().await;
        if let Some(ref release) = *cached {
            let age = (Utc::now().naive_utc() - release.fetched_at).num_seconds();
            if age < CACHE_TTL_SECS {
                let result = serde_json::json!({
                    "latestVersion": release.version,
                    "publishedAt": release.published_at,
                    "htmlUrl": release.html_url,
                    "controllerVersion": env!("CARGO_PKG_VERSION"),
                });
                return (StatusCode::OK, ApiResponse::success(result));
            }
        }
    }

    match fetch_latest_release().await {
        Ok(release) => {
            let result = serde_json::json!({
                "latestVersion": release.version,
                "publishedAt": release.published_at,
                "htmlUrl": release.html_url,
                "controllerVersion": env!("CARGO_PKG_VERSION"),
            });
            *cache.write().await = Some(release);
            (StatusCode::OK, ApiResponse::success(result))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("检查最新版本失败: {}", e)),
        ),
    }
}

/// POST /api/nodes/{id}/update
pub async fn trigger_node_update(
    Path(id): Path<i64>,
    Extension(auth_user): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("未认证".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("仅管理员".to_string()));
    }

    let connected_ids = app_state.node_manager.get_loaded_node_ids().await;
    if !connected_ids.contains(&id) {
        return (StatusCode::BAD_REQUEST, ApiResponse::<serde_json::Value>::error("节点不在线".to_string()));
    }

    match app_state.node_manager.send_software_update(id).await {
        Ok(resp) => {
            let result = serde_json::json!({
                "success": resp.success,
                "error": resp.error,
                "newVersion": resp.new_version,
            });
            (StatusCode::OK, ApiResponse::success(result))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("更新失败: {}", e)),
        ),
    }
}

/// POST /api/clients/{id}/update
pub async fn trigger_client_update(
    Path(id): Path<i64>,
    Extension(auth_user): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("未认证".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("仅管理员".to_string()));
    }

    match app_state.client_stream_manager.send_software_update(id).await {
        Ok(resp) => {
            let result = serde_json::json!({
                "success": resp.success,
                "error": resp.error,
                "newVersion": resp.new_version,
            });
            (StatusCode::OK, ApiResponse::success(result))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("更新失败: {}", e)),
        ),
    }
}

/// POST /api/nodes/batch-update
pub async fn batch_update_nodes(
    Extension(auth_user): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("未认证".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("仅管理员".to_string()));
    }

    let db = get_connection().await;
    let connected_ids = app_state.node_manager.get_loaded_node_ids().await;

    if connected_ids.is_empty() {
        return (StatusCode::OK, ApiResponse::success(serde_json::json!({ "results": [] })));
    }

    // 查询在线节点的名称
    let nodes = match crate::entity::Node::find().all(db).await {
        Ok(n) => n,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<serde_json::Value>::error(format!("查询节点失败: {}", e))),
    };

    let node_map: std::collections::HashMap<i64, String> = nodes
        .into_iter()
        .filter(|n| connected_ids.contains(&n.id))
        .map(|n| (n.id, n.name))
        .collect();

    // 并发更新所有在线节点
    let mut handles = Vec::new();
    for (&node_id, name) in &node_map {
        let nm = app_state.node_manager.clone();
        let name = name.clone();
        handles.push(tokio::spawn(async move {
            let result = nm.send_software_update(node_id).await;
            (node_id, name, result)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok((id, name, Ok(resp))) => {
                results.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "success": resp.success,
                    "error": resp.error,
                    "newVersion": resp.new_version,
                }));
            }
            Ok((id, name, Err(e))) => {
                results.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "success": false,
                    "error": e.to_string(),
                }));
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "success": false,
                    "error": format!("任务执行失败: {}", e),
                }));
            }
        }
    }

    (StatusCode::OK, ApiResponse::success(serde_json::json!({ "results": results })))
}

/// POST /api/clients/batch-update
pub async fn batch_update_clients(
    Extension(auth_user): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("未认证".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("仅管理员".to_string()));
    }

    let db = get_connection().await;

    // 查询所有在线客户端
    let all_clients = match crate::entity::Client::find().all(db).await {
        Ok(c) => c,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<serde_json::Value>::error(format!("查询客户端失败: {}", e))),
    };

    let online_clients: Vec<_> = {
        let check = app_state.client_stream_manager.check_all_clients().await;
        let online_set: std::collections::HashSet<i64> = check.into_iter().filter(|(_, online)| *online).map(|(id, _)| id).collect();
        all_clients.into_iter().filter(|c| online_set.contains(&c.id)).collect()
    };

    if online_clients.is_empty() {
        return (StatusCode::OK, ApiResponse::success(serde_json::json!({ "results": [] })));
    }

    // 并发更新所有在线客户端
    let mut handles = Vec::new();
    for client in &online_clients {
        let csm = app_state.client_stream_manager.clone();
        let client_id = client.id;
        let name = client.name.clone();
        handles.push(tokio::spawn(async move {
            let result = csm.send_software_update(client_id).await;
            (client_id, name, result)
        }));
    }

    let mut results = Vec::new();
    for handle in handles {
        match handle.await {
            Ok((id, name, Ok(resp))) => {
                results.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "success": resp.success,
                    "error": resp.error,
                    "newVersion": resp.new_version,
                }));
            }
            Ok((id, name, Err(e))) => {
                results.push(serde_json::json!({
                    "id": id,
                    "name": name,
                    "success": false,
                    "error": e.to_string(),
                }));
            }
            Err(e) => {
                results.push(serde_json::json!({
                    "success": false,
                    "error": format!("任务执行失败: {}", e),
                }));
            }
        }
    }

    (StatusCode::OK, ApiResponse::success(serde_json::json!({ "results": results })))
}
