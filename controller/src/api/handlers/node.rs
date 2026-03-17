use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, PaginatorTrait, QueryFilter, Set};
use serde::Deserialize;
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    entity::{Node, node},
    migration::get_connection,
    middleware::AuthUser,
    AppState,
};

use super::ApiResponse;

#[derive(Deserialize)]
pub struct CreateNodeRequest {
    pub name: String,
    pub url: String,
    pub secret: Option<String>,
    pub region: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "tunnelAddr")]
    pub tunnel_addr: Option<String>,
    #[serde(rename = "tunnelPort")]
    pub tunnel_port: Option<i32>,
    #[serde(rename = "tunnelProtocol")]
    pub tunnel_protocol: Option<String>,
    #[serde(rename = "kcpConfig")]
    pub kcp_config: Option<String>,
    #[serde(rename = "nodeType")]
    pub node_type: Option<String>,
    #[serde(rename = "maxProxyCount")]
    pub max_proxy_count: Option<i32>,
    #[serde(rename = "allowedPortRange")]
    pub allowed_port_range: Option<String>,
    #[serde(rename = "trafficQuotaGb")]
    pub traffic_quota_gb: Option<f64>,
    #[serde(rename = "trafficResetCycle")]
    pub traffic_reset_cycle: Option<String>,
    #[serde(rename = "speedLimit")]
    pub speed_limit: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateNodeRequest {
    pub name: Option<String>,
    pub url: Option<String>,
    pub secret: Option<String>,
    pub region: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "tunnelAddr")]
    pub tunnel_addr: Option<String>,
    #[serde(rename = "tunnelPort")]
    pub tunnel_port: Option<i32>,
    #[serde(rename = "tunnelProtocol")]
    pub tunnel_protocol: Option<String>,
    #[serde(rename = "kcpConfig")]
    pub kcp_config: Option<String>,
    #[serde(rename = "nodeType")]
    pub node_type: Option<String>,
    #[serde(rename = "maxProxyCount")]
    pub max_proxy_count: Option<Option<i32>>,
    #[serde(rename = "allowedPortRange")]
    pub allowed_port_range: Option<Option<String>>,
    #[serde(rename = "trafficQuotaGb")]
    pub traffic_quota_gb: Option<Option<f64>>,
    #[serde(rename = "trafficResetCycle")]
    pub traffic_reset_cycle: Option<String>,
    #[serde(rename = "speedLimit")]
    pub speed_limit: Option<Option<i64>>,
}

/// GET /api/nodes — 列出节点（管理员看全部，普通用户看可用的）
pub async fn list_nodes(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<node::Model>>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    if auth_user.is_admin {
        // 管理员可以看到所有节点
        match Node::find().all(db).await {
            Ok(nodes) => (StatusCode::OK, ApiResponse::success(nodes)),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<Vec<node::Model>>::error(format!("Failed to list nodes: {}", e)),
            ),
        }
    } else {
        // 普通用户只能看到共享节点 + 自己的独享节点
        match Node::find().all(db).await {
            Ok(all_nodes) => {
                // 获取用户的独享节点
                let user_node_ids = match crate::entity::UserNode::find()
                    .filter(crate::entity::user_node::Column::UserId.eq(auth_user.id))
                    .all(db)
                    .await
                {
                    Ok(user_nodes) => user_nodes.into_iter().map(|un| un.node_id).collect::<Vec<_>>(),
                    Err(_) => vec![],
                };

                // 过滤出共享节点 + 用户的独享节点
                let available_nodes: Vec<node::Model> = all_nodes
                    .into_iter()
                    .filter(|node| node.node_type == "shared" || user_node_ids.contains(&node.id))
                    .collect();

                (StatusCode::OK, ApiResponse::success(available_nodes))
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<Vec<node::Model>>::error(format!("Failed to list nodes: {}", e)),
            ),
        }
    }
}

/// POST /api/nodes — 创建节点
pub async fn create_node(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(_app_state): Extension<AppState>,
    Json(req): Json<CreateNodeRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<node::Model>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<node::Model>::error("Only admin can manage nodes".to_string()));
    }

    let now = Utc::now().naive_utc();
    let new_node = node::ActiveModel {
        id: NotSet,
        name: Set(req.name),
        url: Set(req.url.clone()),
        secret: Set(req.secret.unwrap_or_else(|| Uuid::new_v4().to_string())),
        is_online: Set(false),
        region: Set(req.region),
        public_ip: Set(None),
        description: Set(req.description),
        tunnel_addr: Set(req.tunnel_addr.unwrap_or_default()),
        tunnel_port: Set(req.tunnel_port.unwrap_or(7000)),
        tunnel_protocol: Set(req.tunnel_protocol.unwrap_or_else(|| "quic".to_string())),
        kcp_config: Set(req.kcp_config),
        node_type: Set(req.node_type.unwrap_or_else(|| "shared".to_string())),
        max_proxy_count: Set(req.max_proxy_count),
        allowed_port_range: Set(req.allowed_port_range),
        traffic_quota_gb: Set(req.traffic_quota_gb),
        traffic_reset_cycle: Set(req.traffic_reset_cycle.unwrap_or_else(|| "none".to_string())),
        total_bytes_sent: Set(0),
        total_bytes_received: Set(0),
        last_reset_at: Set(None),
        is_traffic_exceeded: Set(false),
        speed_limit: Set(req.speed_limit),
        version: Set(None),
        last_online_at: NotSet,
        created_at: Set(now),
        updated_at: Set(now),
    };

    let db = get_connection().await;
    match new_node.insert(db).await {
        Ok(node_model) => {
            // gRPC 模式下节点会主动连接认证，无需手动添加
            (StatusCode::OK, ApiResponse::success(node_model))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<node::Model>::error(format!("Failed to create node: {}", e)),
        ),
    }
}

/// GET /api/nodes/{id} — 获取节点详情
pub async fn get_node(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<node::Model>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<node::Model>::error("Only admin can manage nodes".to_string()));
    }

    let db = get_connection().await;
    match Node::find_by_id(id).one(db).await {
        Ok(Some(node_model)) => (StatusCode::OK, ApiResponse::success(node_model)),
        Ok(None) => (StatusCode::NOT_FOUND, ApiResponse::<node::Model>::error("Node not found".to_string())),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<node::Model>::error(format!("Failed to get node: {}", e)),
        ),
    }
}

/// PUT /api/nodes/{id} — 更新节点
pub async fn update_node(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(req): Json<UpdateNodeRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<node::Model>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<node::Model>::error("Only admin can manage nodes".to_string()));
    }

    let db = get_connection().await;
    let node_model = match Node::find_by_id(id).one(db).await {
        Ok(Some(n)) => n,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<node::Model>::error("Node not found".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<node::Model>::error(format!("Failed to find node: {}", e))),
    };

    // 保存旧的协议值，用于检测变更
    let old_protocol = node_model.tunnel_protocol.clone();
    let old_speed_limit = node_model.speed_limit;
    let new_protocol_opt = req.tunnel_protocol.clone();

    let mut active: node::ActiveModel = node_model.into();

    if let Some(name) = req.name {
        active.name = Set(name);
    }
    if let Some(url) = req.url {
        active.url = Set(url);
    }
    if let Some(secret) = req.secret {
        active.secret = Set(secret);
    }
    if req.region.is_some() {
        active.region = Set(req.region);
    }
    if req.description.is_some() {
        active.description = Set(req.description);
    }
    if let Some(tunnel_addr) = req.tunnel_addr {
        active.tunnel_addr = Set(tunnel_addr);
    }
    if let Some(tunnel_port) = req.tunnel_port {
        active.tunnel_port = Set(tunnel_port);
    }
    if let Some(ref tunnel_protocol) = new_protocol_opt {
        active.tunnel_protocol = Set(tunnel_protocol.clone());
    }
    if req.kcp_config.is_some() {
        active.kcp_config = Set(req.kcp_config);
    }
    if let Some(node_type) = req.node_type {
        active.node_type = Set(node_type);
    }
    if let Some(max_proxy_count) = req.max_proxy_count {
        active.max_proxy_count = Set(max_proxy_count);
    }
    if let Some(allowed_port_range) = req.allowed_port_range {
        active.allowed_port_range = Set(allowed_port_range);
    }
    if let Some(traffic_quota_gb) = req.traffic_quota_gb {
        active.traffic_quota_gb = Set(traffic_quota_gb);
    }
    if let Some(traffic_reset_cycle) = req.traffic_reset_cycle {
        active.traffic_reset_cycle = Set(traffic_reset_cycle);
    }
    if let Some(speed_limit) = req.speed_limit {
        active.speed_limit = Set(speed_limit);
    }
    active.updated_at = Set(Utc::now().naive_utc());

    match active.update(db).await {
        Ok(updated) => {
            // 检查协议是否变更
            if let Some(ref new_protocol) = new_protocol_opt {
                if new_protocol != &old_protocol {
                    info!("节点 #{} 协议变更: {} -> {}", id, old_protocol, new_protocol);

                    // 检查节点是否在线
                    let connected_ids = app_state.node_manager.get_loaded_node_ids().await;
                    if connected_ids.contains(&id) {
                        // 推送协议更新到在线节点
                        if let Err(e) = app_state.node_manager.send_update_protocol(id, new_protocol).await {
                            warn!("推送协议更新到节点 #{} 失败: {}", id, e);
                        } else {
                            info!("已推送协议更新到节点 #{}", id);
                        }
                    }

                    // 通知该节点上的所有客户端刷新配置
                    app_state.client_stream_manager.notify_clients_for_node(id).await;
                }
            }

            // gRPC 模式下节点会主动重连，无需手动更新连接

            // 如果 speed_limit 变更，推送到在线节点
            if updated.speed_limit != old_speed_limit {
                let connected_ids = app_state.node_manager.get_loaded_node_ids().await;
                if connected_ids.contains(&id) {
                    let new_limit = updated.speed_limit.unwrap_or(0);
                    if let Err(e) = app_state.node_manager.send_update_speed_limit(id, new_limit).await {
                        warn!("推送速度限制到节点 #{} 失败: {}", id, e);
                    } else {
                        info!("已推送速度限制到节点 #{}: {} bytes/s", id, new_limit);
                    }
                }
            }

            (StatusCode::OK, ApiResponse::success(updated))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<node::Model>::error(format!("Failed to update node: {}", e))),
    }
}

/// DELETE /api/nodes/{id} — 删除节点（需无关联客户端）
pub async fn delete_node(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(_app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<&str>::error("Only admin can manage nodes".to_string()));
    }

    let db = get_connection().await;

    // 检查是否有关联代理（proxy 仍然可以指定节点）
    let proxy_count = crate::entity::Proxy::find()
        .filter(crate::entity::proxy::Column::NodeId.eq(id))
        .count(db)
        .await
        .unwrap_or(0);

    if proxy_count > 0 {
        return (
            StatusCode::BAD_REQUEST,
            ApiResponse::<&str>::error(format!("无法删除节点：仍有 {} 个代理关联到此节点", proxy_count)),
        );
    }

    match Node::delete_by_id(id).exec(db).await {
        Ok(_) => {
            // gRPC 模式下节点断开后会自动清理
            (StatusCode::OK, ApiResponse::success("Node deleted successfully"))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<&str>::error(format!("Failed to delete node: {}", e)),
        ),
    }
}

/// POST /api/nodes/{id}/test — 测试节点连接
pub async fn test_node_connection(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("Only admin can manage nodes".to_string()));
    }

    let db = get_connection().await;
    let node_model = match Node::find_by_id(id).one(db).await {
        Ok(Some(n)) => n,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<serde_json::Value>::error("Node not found".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<serde_json::Value>::error(format!("Failed to find node: {}", e))),
    };

    // gRPC 模式下检查节点是否已连接
    let connected_ids = app_state.node_manager.get_loaded_node_ids().await;
    let is_online = connected_ids.contains(&id);

    let result = serde_json::json!({
        "online": is_online,
        "node_name": node_model.name,
    });
    (StatusCode::OK, ApiResponse::success(result))
}

/// GET /api/nodes/{id}/status — 获取节点实时状态
pub async fn get_node_status(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("Only admin can manage nodes".to_string()));
    }

    // gRPC 模式下检查节点是否已连接
    let connected_ids = app_state.node_manager.get_loaded_node_ids().await;
    if !connected_ids.contains(&id) {
        return (StatusCode::NOT_FOUND, ApiResponse::<serde_json::Value>::error("Node not connected via gRPC".to_string()));
    }

    // 通过 ProxyControl 获取状态（会通过 gRPC 流发送命令）
    match app_state.proxy_control.get_server_status().await {
        Ok(status) => {
            let result = serde_json::json!({
                "connected_clients": status.connected_clients,
                "active_proxy_count": status.active_proxy_count,
            });
            (StatusCode::OK, ApiResponse::success(result))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("Failed to get node status: {}", e)),
        ),
    }
}

#[derive(Deserialize)]
pub struct GetNodeLogsQuery {
    #[serde(default = "default_log_lines")]
    lines: u32,
}

fn default_log_lines() -> u32 {
    100
}

/// GET /api/nodes/{id}/logs — 获取节点日志（仅管理员）
pub async fn get_node_logs(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    axum::extract::Query(query): axum::extract::Query<GetNodeLogsQuery>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<serde_json::Value>::error("Not authenticated".to_string())),
    };

    if !auth_user.is_admin {
        return (StatusCode::FORBIDDEN, ApiResponse::<serde_json::Value>::error("Only admin can view node logs".to_string()));
    }

    let db = get_connection().await;
    let node_model = match Node::find_by_id(id).one(db).await {
        Ok(Some(n)) => n,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<serde_json::Value>::error("Node not found".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<serde_json::Value>::error(format!("Failed to find node: {}", e))),
    };

    // 检查节点是否在线
    let connected_ids = app_state.node_manager.get_loaded_node_ids().await;
    if !connected_ids.contains(&id) {
        return (StatusCode::BAD_REQUEST, ApiResponse::<serde_json::Value>::error("Node is offline, cannot retrieve logs".to_string()));
    }

    // 通过 gRPC 获取节点日志
    match app_state.node_manager.get_node_logs(id, query.lines).await {
        Ok(logs) => {
            let result = serde_json::json!({
                "node_id": id,
                "node_name": node_model.name,
                "logs": logs,
            });
            (StatusCode::OK, ApiResponse::success(result))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<serde_json::Value>::error(format!("Failed to get node logs: {}", e)),
        ),
    }
}
