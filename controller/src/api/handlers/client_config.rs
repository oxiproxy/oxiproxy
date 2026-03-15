use axum::{
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use tracing::debug;

use common::protocol::client_config::{
    ClientConnectConfig, ClientConnectConfigRequest,
};
use common::KcpConfig;
use common::TunnelProtocol;

use crate::{
    entity::{Client, Node, client},
    migration::get_connection,
};

/// POST /api/client/connect-config — 客户端获取连接配置（公开端点，token 认证）
pub async fn get_client_connect_config(
    Json(req): Json<ClientConnectConfigRequest>,
) -> impl IntoResponse {
    let db = get_connection().await;

    // 1. 通过 token 查找客户端
    let client_model = match Client::find()
        .filter(client::Column::Token.eq(&req.token))
        .one(db)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error": "invalid_token",
                    "message": "无效的 token"
                })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "db_error",
                    "message": format!("数据库查询失败: {}", e)
                })),
            );
        }
    };

    // 2. 检查流量是否超限
    if client_model.is_traffic_exceeded {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "traffic_exceeded",
                "message": format!("客户端 '{}' 流量已超限", client_model.name)
            })),
        );
    }

    // 3. 查找客户端的第一个启用的代理，并获取其节点配置
    let proxies = match crate::entity::Proxy::find()
        .filter(crate::entity::proxy::Column::ClientId.eq(client_model.id.to_string()))
        .filter(crate::entity::proxy::Column::Enabled.eq(true))
        .all(db)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "db_error",
                    "message": format!("数据库查询失败: {}", e)
                })),
            );
        }
    };

    // 找到第一个指定了节点的代理
    let node_id = proxies.iter()
        .find_map(|p| p.node_id)
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "no_node_assigned",
                    "message": format!("客户端 '{}' (ID: {}) 的代理未分配节点，请在管理面板中为代理分配节点", client_model.name, client_model.id)
                })),
            )
        });

    let node_id = match node_id {
        Ok(id) => id,
        Err(e) => return e,
    };

    // 4. 查找节点
    let node_model = match Node::find_by_id(node_id).one(db).await {
        Ok(Some(n)) => n,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "node_not_found",
                    "message": format!("分配的节点 (ID: {}) 不存在", node_id)
                })),
            );
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "db_error",
                    "message": format!("数据库查询失败: {}", e)
                })),
            );
        }
    };

    // 5. 构建连接配置
    let protocol = match node_model.tunnel_protocol.as_str() {
        "kcp" => TunnelProtocol::Kcp,
        "tcp" => TunnelProtocol::Tcp,
        _ => TunnelProtocol::Quic,
    };

    let kcp = node_model.kcp_config
        .and_then(|s| serde_json::from_str::<KcpConfig>(&s).ok());

    let config = ClientConnectConfig {
        server_addr: node_model.tunnel_addr,
        server_port: node_model.tunnel_port as u16,
        protocol,
        kcp,
        client_id: client_model.id,
        client_name: client_model.name,
    };

    debug!("客户端 {} (ID: {}) 获取连接配置: 节点 {} ({}:{})",
        config.client_name, config.client_id, node_model.name, config.server_addr, config.server_port);

    (StatusCode::OK, Json(serde_json::to_value(config).expect("failed to serialize config")))
}
