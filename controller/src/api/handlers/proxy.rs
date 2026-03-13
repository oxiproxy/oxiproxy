use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::{IntoResponse, Json},
};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, NotSet, QueryFilter, Set};
use serde::Deserialize;
use tracing::info;
use uuid::Uuid;

use crate::{entity::Proxy, migration::get_connection, middleware::AuthUser, AppState};

use super::ApiResponse;

#[derive(Deserialize)]
pub struct CreateProxyRequest {
    pub client_id: String,  // 改为 String 以兼容前端
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    #[serde(rename = "localIP")]
    pub local_ip: String,
    #[serde(rename = "localPort")]
    pub local_port: u16,
    #[serde(rename = "remotePort")]
    pub remote_port: u16,
    #[serde(rename = "nodeId")]
    pub node_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct UpdateProxyRequest {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub proxy_type: Option<String>,
    #[serde(rename = "localIP")]
    pub local_ip: Option<String>,
    #[serde(rename = "localPort")]
    pub local_port: Option<u16>,
    #[serde(rename = "remotePort")]
    pub remote_port: Option<u16>,
    pub enabled: Option<bool>,
}

pub async fn list_proxies(Extension(auth_user_opt): Extension<Option<AuthUser>>) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("Not authenticated".to_string())),
    };
    let db = get_connection().await;

    let proxies = if auth_user.is_admin {
        // Admin can see all proxies
        match Proxy::find().all(db).await {
            Ok(proxies) => proxies,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!(
                        "Failed to list proxies: {}",
                        e
                    )),
                )
            }
        }
    } else {
        // Regular users can only see proxies for their own clients
        let client_ids = match crate::entity::Client::find()
            .filter(crate::entity::client::Column::UserId.eq(auth_user.id))
            .all(db)
            .await
        {
            Ok(clients) => clients.into_iter().map(|c| c.id.to_string()).collect::<Vec<_>>(),
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!(
                        "Failed to get clients: {}",
                        e
                    )),
                )
            }
        };

        if client_ids.is_empty() {
            vec![]
        } else {
            // Get proxies for those clients
            match Proxy::find()
                .filter(crate::entity::proxy::Column::ClientId.is_in(client_ids))
                .all(db)
                .await
            {
                Ok(proxies) => proxies,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!(
                            "Failed to list proxies: {}",
                            e
                        )),
                    )
                }
            }
        }
    };

    (StatusCode::OK, ApiResponse::success(proxies))
}

pub async fn list_proxies_by_client(
    Path(client_id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("Not authenticated".to_string())),
    };
    let db = get_connection().await;

    // Check if user has access to this client (via node binding)
    if !auth_user.is_admin {
        // First get the client's node_id
        let client = match crate::entity::Client::find_by_id(client_id).one(db).await {
            Ok(Some(c)) => c,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    ApiResponse::<Vec<crate::entity::proxy::Model>>::error(
                        "Client not found".to_string(),
                    ),
                )
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!(
                        "Failed to check access: {}",
                        e
                    )),
                )
            }
        };

        // Check if user owns the client
        if client.user_id != Some(auth_user.id) {
            return (
                StatusCode::FORBIDDEN,
                ApiResponse::<Vec<crate::entity::proxy::Model>>::error(
                    "Access denied to this client".to_string(),
                ),
            )
        }
    }

    match Proxy::find()
        .filter(crate::entity::proxy::Column::ClientId.eq(client_id.to_string()))
        .all(db)
        .await
    {
        Ok(proxies) => (StatusCode::OK, ApiResponse::success(proxies)),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!(
                "Failed to list proxies: {}",
                e
            )),
        ),
    }
}

pub async fn create_proxy(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(req): Json<CreateProxyRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<crate::entity::proxy::Model>::error("未认证".to_string())),
    };

    let db = get_connection().await;

    // 解析 client_id
    let parsed_client_id: i64 = match req.client_id.parse() {
        Ok(id) => id,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                ApiResponse::<crate::entity::proxy::Model>::error(format!("无效的客户端 ID: {}", req.client_id)),
            )
        }
    };

    // 获取客户端信息以验证端口限制
    let client = match crate::entity::Client::find_by_id(parsed_client_id)
        .one(db)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                ApiResponse::<crate::entity::proxy::Model>::error("客户端不存在".to_string()),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<crate::entity::proxy::Model>::error(format!("查询客户端失败: {}", e)),
            )
        }
    };

    // 验证端口限制（仅对非管理员用户）
    if !auth_user.is_admin {
        if let Some(user_id) = client.user_id {
            match crate::port_limiter::validate_user_port_limit(user_id, req.remote_port, db).await {
                Ok((allowed, reason)) => {
                    if !allowed {
                        return (
                            StatusCode::FORBIDDEN,
                            ApiResponse::<crate::entity::proxy::Model>::error(reason),
                        );
                    }
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiResponse::<crate::entity::proxy::Model>::error(format!("验证端口限制失败: {}", e)),
                    );
                }
            }
        }
    }

    // 验证节点权限
    if let Some(node_id) = req.node_id {
        // 获取节点信息
        let node = match crate::entity::Node::find_by_id(node_id).one(db).await {
            Ok(Some(n)) => n,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    ApiResponse::<crate::entity::proxy::Model>::error("节点不存在".to_string()),
                )
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<crate::entity::proxy::Model>::error(format!("查询节点失败: {}", e)),
                )
            }
        };

        // 如果是独享节点，需要检查用户是否有权限
        if node.node_type == "dedicated" && !auth_user.is_admin {
            // 检查客户端是否属于当前用户（复用已查询的 client）
            if client.user_id != Some(auth_user.id) {
                return (
                    StatusCode::FORBIDDEN,
                    ApiResponse::<crate::entity::proxy::Model>::error("无权访问此客户端".to_string()),
                );
            }

            // 检查节点是否分配给了该用户
            let user_node = crate::entity::UserNode::find()
                .filter(crate::entity::user_node::Column::UserId.eq(auth_user.id))
                .filter(crate::entity::user_node::Column::NodeId.eq(node_id))
                .one(db)
                .await;

            match user_node {
                Ok(Some(_)) => {
                    // 用户有权限使用此独享节点
                }
                Ok(None) => {
                    return (
                        StatusCode::FORBIDDEN,
                        ApiResponse::<crate::entity::proxy::Model>::error("此独享节点未分配给您，无法使用".to_string()),
                    );
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        ApiResponse::<crate::entity::proxy::Model>::error(format!("检查节点权限失败: {}", e)),
                    );
                }
            }
        }
        // 共享节点对所有用户可用，无需额外检查
    }

    // 验证节点限制（代理数量、端口范围、流量）
    if let Some(node_id) = req.node_id {
        match crate::node_limiter::validate_node_proxy_limit(node_id, req.remote_port, db).await {
            Ok((allowed, reason)) => {
                if !allowed {
                    return (
                        StatusCode::FORBIDDEN,
                        ApiResponse::<crate::entity::proxy::Model>::error(reason),
                    );
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<crate::entity::proxy::Model>::error(format!(
                        "验证节点限制失败: {}",
                        e
                    )),
                );
            }
        }
    }

    // 检查端口是否已被占用（同一节点上的 remote_port 必须唯一）
    {
        let mut port_query = Proxy::find()
            .filter(crate::entity::proxy::Column::RemotePort.eq(req.remote_port))
            .filter(crate::entity::proxy::Column::Enabled.eq(true));

        if let Some(node_id) = req.node_id {
            port_query =
                port_query.filter(crate::entity::proxy::Column::NodeId.eq(node_id));
        } else {
            port_query =
                port_query.filter(crate::entity::proxy::Column::NodeId.is_null());
        }

        match port_query.one(db).await {
            Ok(Some(existing)) => {
                return (
                    StatusCode::CONFLICT,
                    ApiResponse::<crate::entity::proxy::Model>::error(format!(
                        "远程端口 {} 已被代理「{}」占用",
                        req.remote_port, existing.name
                    )),
                );
            }
            Ok(None) => {} // 端口未被占用
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<crate::entity::proxy::Model>::error(format!(
                        "检查端口占用失败: {}",
                        e
                    )),
                );
            }
        }
    }

    let now = chrono::Utc::now().naive_utc();

    let new_proxy = crate::entity::proxy::ActiveModel {
        id: NotSet,
        client_id: Set(req.client_id.clone()),
        name: Set(req.name),
        proxy_type: Set(req.proxy_type),
        local_ip: Set(req.local_ip),
        local_port: Set(req.local_port),
        remote_port: Set(req.remote_port),
        enabled: Set(true),
        node_id: Set(req.node_id),
        group_id: Set(None),
        total_bytes_sent: Set(0),
        total_bytes_received: Set(0),
        created_at: Set(now),
        updated_at: Set(now),
    };
    let db = get_connection().await;
    match new_proxy.insert(db).await {
        Ok(proxy) => {
            info!("代理已创建: {} (ID: {}, 客户端: {})", proxy.name, proxy.id, proxy.client_id);

            // 通过 ProxyControl trait 动态启动代理监听器（同步等待，检测端口占用）
            if let Err(e) = app_state.proxy_control.start_proxy(&req.client_id, proxy.id).await {
                // 启动失败（可能端口被占用），回滚删除数据库记录
                tracing::warn!("启动代理监听器失败，回滚创建: {}", e);
                let _ = Proxy::delete_by_id(proxy.id).exec(db).await;
                return (
                    StatusCode::CONFLICT,
                    ApiResponse::<crate::entity::proxy::Model>::error(format!(
                        "启动代理监听器失败: {}",
                        e
                    )),
                );
            }

            info!("代理监听器已动态启动: {}", proxy.name);

            // 通知 Agent Client 代理配置已变更
            let csm = app_state.client_stream_manager.clone();
            let client_id_notify = req.client_id.clone();
            tokio::spawn(async move {
                csm.notify_proxy_change(&client_id_notify).await;
            });

            (StatusCode::OK, ApiResponse::success(proxy))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<crate::entity::proxy::Model>::error(format!(
                "Failed to create proxy: {}",
                e
            )),
        ),
    }
}

pub async fn update_proxy(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(req): Json<UpdateProxyRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<crate::entity::proxy::Model>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;
    match Proxy::find_by_id(id).one(db).await {
        Ok(Some(proxy)) => {
            // 校验归属权
            if let Err((status, msg)) = super::verify_proxy_ownership(&auth_user, &proxy, db).await {
                return (status, ApiResponse::<crate::entity::proxy::Model>::error(msg));
            }

            let old_enabled = proxy.enabled;
            let old_proxy_type = proxy.proxy_type.clone();
            let old_local_ip = proxy.local_ip.clone();
            let old_local_port = proxy.local_port;
            let old_remote_port = proxy.remote_port;
            let proxy_node_id = proxy.node_id;
            let client_id = proxy.client_id.clone();
            let mut proxy: crate::entity::proxy::ActiveModel = proxy.into();

            let mut config_changed = false;

            if let Some(name) = req.name {
                proxy.name = Set(name);
            }
            if let Some(proxy_type) = req.proxy_type {
                if proxy_type != old_proxy_type {
                    config_changed = true;
                }
                proxy.proxy_type = Set(proxy_type);
            }
            if let Some(local_ip) = req.local_ip {
                if local_ip != old_local_ip {
                    config_changed = true;
                }
                proxy.local_ip = Set(local_ip);
            }
            if let Some(local_port) = req.local_port {
                if local_port != old_local_port {
                    config_changed = true;
                }
                proxy.local_port = Set(local_port);
            }
            if let Some(remote_port) = req.remote_port {
                if remote_port != old_remote_port {
                    // 验证节点端口范围限制
                    if let Some(node_id) = proxy_node_id {
                        match crate::node_limiter::validate_node_proxy_limit(
                            node_id,
                            remote_port,
                            db,
                        )
                        .await
                        {
                            Ok((allowed, reason)) => {
                                if !allowed {
                                    return (
                                        StatusCode::FORBIDDEN,
                                        ApiResponse::<crate::entity::proxy::Model>::error(
                                            reason,
                                        ),
                                    );
                                }
                            }
                            Err(e) => {
                                return (
                                    StatusCode::INTERNAL_SERVER_ERROR,
                                    ApiResponse::<crate::entity::proxy::Model>::error(
                                        format!("验证节点限制失败: {}", e),
                                    ),
                                );
                            }
                        }
                    }

                    // 检查新端口是否已被占用（排除当前代理自身）
                    let mut port_query = Proxy::find()
                        .filter(crate::entity::proxy::Column::RemotePort.eq(remote_port))
                        .filter(crate::entity::proxy::Column::Enabled.eq(true))
                        .filter(crate::entity::proxy::Column::Id.ne(id));

                    if let Some(node_id) = proxy_node_id {
                        port_query = port_query
                            .filter(crate::entity::proxy::Column::NodeId.eq(node_id));
                    } else {
                        port_query = port_query
                            .filter(crate::entity::proxy::Column::NodeId.is_null());
                    }

                    match port_query.one(db).await {
                        Ok(Some(existing)) => {
                            return (
                                StatusCode::CONFLICT,
                                ApiResponse::<crate::entity::proxy::Model>::error(
                                    format!(
                                        "远程端口 {} 已被代理「{}」占用",
                                        remote_port, existing.name
                                    ),
                                ),
                            );
                        }
                        Ok(None) => {} // 端口未被占用
                        Err(e) => {
                            return (
                                StatusCode::INTERNAL_SERVER_ERROR,
                                ApiResponse::<crate::entity::proxy::Model>::error(
                                    format!("检查端口占用失败: {}", e),
                                ),
                            );
                        }
                    }

                    config_changed = true;
                }
                proxy.remote_port = Set(remote_port);
            }

            let enabled_changed = if let Some(enabled) = req.enabled {
                proxy.enabled = Set(enabled);
                old_enabled != enabled
            } else {
                false
            };

            proxy.updated_at = Set(chrono::Utc::now().naive_utc());

            match proxy.update(&*db).await {
                Ok(updated) => {
                    info!("代理已更新: {} (ID: {})", updated.name, updated.id);

                    let need_restart = enabled_changed || (config_changed && updated.enabled);

                    if need_restart {
                        // 先停止旧监听器
                        if let Err(e) = app_state.proxy_control.stop_proxy(&client_id, updated.id).await {
                            tracing::warn!("停止旧代理监听器: {}", e);
                        }

                        if updated.enabled {
                            // 同步启动新监听器，检测端口占用
                            if let Err(e) = app_state.proxy_control.start_proxy(&client_id, updated.id).await {
                                tracing::error!("启动代理监听器失败: {}", e);

                                // 如果是端口变更导致启动失败，回滚 remote_port
                                if config_changed && req.remote_port.is_some() {
                                    let mut revert: crate::entity::proxy::ActiveModel = updated.into();
                                    revert.remote_port = Set(old_remote_port);
                                    revert.updated_at = Set(chrono::Utc::now().naive_utc());
                                    let _ = revert.update(&*db).await;
                                }

                                return (
                                    StatusCode::CONFLICT,
                                    ApiResponse::<crate::entity::proxy::Model>::error(format!(
                                        "启动代理监听器失败: {}",
                                        e
                                    )),
                                );
                            }
                            info!("代理监听器已重启: {}", updated.name);
                        } else {
                            info!("代理监听器已停止: {}", updated.name);
                        }
                    }

                    // 通知 Agent Client 代理配置已变更
                    if enabled_changed || config_changed {
                        let csm = app_state.client_stream_manager.clone();
                        let client_id_notify = client_id.clone();
                        tokio::spawn(async move {
                            csm.notify_proxy_change(&client_id_notify).await;
                        });
                    }

                    (StatusCode::OK, ApiResponse::success(updated))
                }
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    ApiResponse::<crate::entity::proxy::Model>::error(format!(
                        "Failed to update proxy: {}",
                        e
                    )),
                ),
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            ApiResponse::<crate::entity::proxy::Model>::error("Proxy not found".to_string()),
        ),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<crate::entity::proxy::Model>::error(format!(
                "Failed to get proxy: {}",
                e
            )),
        ),
    }
}

pub async fn delete_proxy(
    Path(id): Path<i64>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    // 先获取代理信息，用于停止监听器
    let proxy = match Proxy::find_by_id(id).one(db).await {
        Ok(Some(p)) => p,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                ApiResponse::<&str>::error("Proxy not found".to_string()),
            )
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                ApiResponse::<&str>::error(format!("Failed to get proxy: {}", e)),
            )
        }
    };

    // 校验归属权
    if let Err((status, msg)) = super::verify_proxy_ownership(&auth_user, &proxy, db).await {
        return (status, ApiResponse::<&str>::error(msg));
    }

    let client_id = proxy.client_id.clone();
    let proxy_name = proxy.name.clone();

    // 删除代理
    match Proxy::delete_by_id(id).exec(db).await {
        Ok(_) => {
            info!("代理已删除: {} (ID: {})", proxy_name, id);

            // 通过 ProxyControl trait 停止代理监听器
            let proxy_control = app_state.proxy_control.clone();
            tokio::spawn(async move {
                if let Err(e) = proxy_control.stop_proxy(&client_id, id).await {
                    tracing::error!("停止代理监听器失败: {}", e);
                } else {
                    info!("代理监听器已停止: {}", proxy_name);
                }
            });

            // 通知 Agent Client 代理配置已变更
            let csm = app_state.client_stream_manager.clone();
            let client_id_notify = proxy.client_id.clone();
            tokio::spawn(async move {
                csm.notify_proxy_change(&client_id_notify).await;
            });

            (StatusCode::OK, ApiResponse::success("Proxy deleted successfully"))
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            ApiResponse::<&str>::error(format!("Failed to delete proxy: {}", e)),
        ),
    }
}

// ============ 批量创建 / 分组操作 ============

#[derive(Deserialize)]
pub struct BatchCreateProxyRequest {
    pub client_id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub proxy_type: String,
    #[serde(rename = "localIP")]
    pub local_ip: String,
    #[serde(rename = "localPorts")]
    pub local_ports: Vec<u16>,
    #[serde(rename = "remotePorts")]
    pub remote_ports: Vec<u16>,
    #[serde(rename = "nodeId")]
    pub node_id: Option<i64>,
}

pub async fn batch_create_proxies(
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(req): Json<BatchCreateProxyRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("未认证".to_string())),
    };

    if req.remote_ports.is_empty() {
        return (StatusCode::BAD_REQUEST, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("远程端口列表不能为空".to_string()));
    }

    if req.local_ports.len() != 1 && req.local_ports.len() != req.remote_ports.len() {
        return (StatusCode::BAD_REQUEST, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(
            format!("本地端口数量（{}）必须为 1 或与远程端口数量（{}）一致", req.local_ports.len(), req.remote_ports.len()),
        ));
    }

    let db = get_connection().await;

    // 验证客户端
    let parsed_client_id: i64 = match req.client_id.parse() {
        Ok(id) => id,
        Err(_) => return (StatusCode::BAD_REQUEST, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("无效的客户端 ID: {}", req.client_id))),
    };
    let client = match crate::entity::Client::find_by_id(parsed_client_id)
        .one(db)
        .await
    {
        Ok(Some(c)) => c,
        Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("客户端不存在".to_string())),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("查询客户端失败: {}", e))),
    };

    // 验证端口限制（仅对非管理员）
    if !auth_user.is_admin {
        if let Some(user_id) = client.user_id {
            for &remote_port in &req.remote_ports {
                match crate::port_limiter::validate_user_port_limit(user_id, remote_port, db).await {
                    Ok((allowed, reason)) => {
                        if !allowed {
                            return (StatusCode::FORBIDDEN, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(reason));
                        }
                    }
                    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("验证端口限制失败: {}", e))),
                }
            }
        }
    }

    // 验证节点权限
    if let Some(node_id) = req.node_id {
        let node = match crate::entity::Node::find_by_id(node_id).one(db).await {
            Ok(Some(n)) => n,
            Ok(None) => return (StatusCode::NOT_FOUND, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("节点不存在".to_string())),
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("查询节点失败: {}", e))),
        };

        if node.node_type == "dedicated" && !auth_user.is_admin {
            if client.user_id != Some(auth_user.id) {
                return (StatusCode::FORBIDDEN, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("无权访问此客户端".to_string()));
            }

            let user_node = crate::entity::UserNode::find()
                .filter(crate::entity::user_node::Column::UserId.eq(auth_user.id))
                .filter(crate::entity::user_node::Column::NodeId.eq(node_id))
                .one(db)
                .await;

            match user_node {
                Ok(Some(_)) => {}
                Ok(None) => return (StatusCode::FORBIDDEN, ApiResponse::<Vec<crate::entity::proxy::Model>>::error("此独享节点未分配给您，无法使用".to_string())),
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("检查节点权限失败: {}", e))),
            }
        }
    }

    // 验证所有端口（节点限制 + 端口唯一性）
    for &remote_port in &req.remote_ports {
        if let Some(node_id) = req.node_id {
            match crate::node_limiter::validate_node_proxy_limit(node_id, remote_port, db).await {
                Ok((allowed, reason)) => {
                    if !allowed {
                        return (StatusCode::FORBIDDEN, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(reason));
                    }
                }
                Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("验证节点限制失败: {}", e))),
            }
        }

        // 检查端口唯一性
        let mut port_query = Proxy::find()
            .filter(crate::entity::proxy::Column::RemotePort.eq(remote_port))
            .filter(crate::entity::proxy::Column::Enabled.eq(true));

        if let Some(node_id) = req.node_id {
            port_query = port_query.filter(crate::entity::proxy::Column::NodeId.eq(node_id));
        } else {
            port_query = port_query.filter(crate::entity::proxy::Column::NodeId.is_null());
        }

        match port_query.one(db).await {
            Ok(Some(existing)) => {
                return (StatusCode::CONFLICT, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(
                    format!("远程端口 {} 已被代理「{}」占用", remote_port, existing.name),
                ));
            }
            Ok(None) => {}
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(format!("检查端口占用失败: {}", e))),
        }
    }

    // 所有验证通过，开始创建
    let group_id = if req.remote_ports.len() > 1 {
        Some(Uuid::new_v4().to_string())
    } else {
        None
    };

    let now = chrono::Utc::now().naive_utc();
    let mut created_proxies: Vec<crate::entity::proxy::Model> = Vec::new();

    for (i, &remote_port) in req.remote_ports.iter().enumerate() {
        let local_port = if req.local_ports.len() == 1 { req.local_ports[0] } else { req.local_ports[i] };
        let proxy_name = if req.remote_ports.len() == 1 {
            req.name.clone()
        } else {
            format!("{}-{}", req.name, remote_port)
        };

        let new_proxy = crate::entity::proxy::ActiveModel {
            id: NotSet,
            client_id: Set(req.client_id.clone()),
            name: Set(proxy_name),
            proxy_type: Set(req.proxy_type.clone()),
            local_ip: Set(req.local_ip.clone()),
            local_port: Set(local_port),
            remote_port: Set(remote_port),
            enabled: Set(true),
            node_id: Set(req.node_id),
            group_id: Set(group_id.clone()),
            total_bytes_sent: Set(0),
            total_bytes_received: Set(0),
            created_at: Set(now),
            updated_at: Set(now),
        };

        match new_proxy.insert(db).await {
            Ok(proxy) => {
                // 启动代理监听器
                if let Err(e) = app_state.proxy_control.start_proxy(&req.client_id, proxy.id).await {
                    tracing::warn!("批量创建：启动代理监听器失败，回滚全部: {}", e);
                    // 回滚：删除已创建的所有代理并停止监听器
                    for p in &created_proxies {
                        let _ = app_state.proxy_control.stop_proxy(&req.client_id, p.id).await;
                        let _ = Proxy::delete_by_id(p.id).exec(db).await;
                    }
                    let _ = Proxy::delete_by_id(proxy.id).exec(db).await;
                    return (StatusCode::CONFLICT, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(
                        format!("端口 {} 启动代理监听器失败: {}", remote_port, e),
                    ));
                }
                created_proxies.push(proxy);
            }
            Err(e) => {
                // 回滚已创建的代理
                for p in &created_proxies {
                    let _ = app_state.proxy_control.stop_proxy(&req.client_id, p.id).await;
                    let _ = Proxy::delete_by_id(p.id).exec(db).await;
                }
                return (StatusCode::INTERNAL_SERVER_ERROR, ApiResponse::<Vec<crate::entity::proxy::Model>>::error(
                    format!("创建代理失败: {}", e),
                ));
            }
        }
    }

    info!("批量创建 {} 个代理 (group_id: {:?}, 客户端: {})", created_proxies.len(), group_id, req.client_id);

    // 通知客户端（只通知一次）
    let csm = app_state.client_stream_manager.clone();
    let client_id_notify = req.client_id.clone();
    tokio::spawn(async move {
        csm.notify_proxy_change(&client_id_notify).await;
    });

    (StatusCode::OK, ApiResponse::success(created_proxies))
}

#[derive(Deserialize)]
pub struct ToggleGroupRequest {
    pub enabled: bool,
}

pub async fn toggle_proxy_group(
    Path(group_id): Path<String>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(req): Json<ToggleGroupRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    let proxies = match super::verify_proxy_group_ownership(&auth_user, &group_id, db).await {
        Ok(p) => p,
        Err((status, msg)) => return (status, ApiResponse::<&str>::error(msg)),
    };

    let client_id = proxies[0].client_id.clone();
    let now = chrono::Utc::now().naive_utc();

    for proxy in &proxies {
        let old_enabled = proxy.enabled;
        if old_enabled == req.enabled {
            continue;
        }

        let mut active: crate::entity::proxy::ActiveModel = proxy.clone().into();
        active.enabled = Set(req.enabled);
        active.updated_at = Set(now);

        if let Err(e) = active.update(db).await {
            tracing::error!("更新代理 {} 状态失败: {}", proxy.id, e);
            continue;
        }

        if req.enabled {
            if let Err(e) = app_state.proxy_control.start_proxy(&client_id, proxy.id).await {
                tracing::warn!("启动代理监听器失败 (ID: {}): {}", proxy.id, e);
            }
        } else if let Err(e) = app_state.proxy_control.stop_proxy(&client_id, proxy.id).await {
            tracing::warn!("停止代理监听器失败 (ID: {}): {}", proxy.id, e);
        }
    }

    info!("代理组 {} 已{}", group_id, if req.enabled { "启用" } else { "禁用" });

    // 通知客户端
    let csm = app_state.client_stream_manager.clone();
    let client_id_notify = client_id.clone();
    tokio::spawn(async move {
        csm.notify_proxy_change(&client_id_notify).await;
    });

    (StatusCode::OK, ApiResponse::success("操作成功"))
}

pub async fn delete_proxy_group(
    Path(group_id): Path<String>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    let proxies = match super::verify_proxy_group_ownership(&auth_user, &group_id, db).await {
        Ok(p) => p,
        Err((status, msg)) => return (status, ApiResponse::<&str>::error(msg)),
    };

    let client_id = proxies[0].client_id.clone();
    let count = proxies.len();

    for proxy in &proxies {
        let _ = Proxy::delete_by_id(proxy.id).exec(db).await;

        let proxy_control = app_state.proxy_control.clone();
        let cid = client_id.clone();
        let pid = proxy.id;
        tokio::spawn(async move {
            if let Err(e) = proxy_control.stop_proxy(&cid, pid).await {
                tracing::error!("停止代理监听器失败 (ID: {}): {}", pid, e);
            }
        });
    }

    info!("代理组 {} 已删除（共 {} 个）", group_id, count);

    // 通知客户端
    let csm = app_state.client_stream_manager.clone();
    let client_id_notify = client_id.clone();
    tokio::spawn(async move {
        csm.notify_proxy_change(&client_id_notify).await;
    });

    (StatusCode::OK, ApiResponse::success("代理组删除成功"))
}

#[derive(Deserialize)]
pub struct UpdateGroupRequest {
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub proxy_type: Option<String>,
    #[serde(rename = "localIP")]
    pub local_ip: Option<String>,
    #[serde(rename = "localPort")]
    pub local_port: Option<u16>,
}

pub async fn update_proxy_group(
    Path(group_id): Path<String>,
    Extension(auth_user_opt): Extension<Option<AuthUser>>,
    Extension(app_state): Extension<AppState>,
    Json(req): Json<UpdateGroupRequest>,
) -> impl IntoResponse {
    let auth_user = match auth_user_opt {
        Some(user) => user,
        None => return (StatusCode::UNAUTHORIZED, ApiResponse::<&str>::error("Not authenticated".to_string())),
    };

    let db = get_connection().await;

    let proxies = match super::verify_proxy_group_ownership(&auth_user, &group_id, db).await {
        Ok(p) => p,
        Err((status, msg)) => return (status, ApiResponse::<&str>::error(msg)),
    };

    let client_id = proxies[0].client_id.clone();
    let now = chrono::Utc::now().naive_utc();
    let mut config_changed = false;

    for proxy in &proxies {
        let mut active: crate::entity::proxy::ActiveModel = proxy.clone().into();
        let mut changed = false;

        if let Some(ref name) = req.name {
            // 更新名称：保留 -port 后缀
            let new_name = if proxies.len() > 1 {
                format!("{}-{}", name, proxy.remote_port)
            } else {
                name.clone()
            };
            active.name = Set(new_name);
            changed = true;
        }
        if let Some(ref proxy_type) = req.proxy_type {
            if proxy_type != &proxy.proxy_type {
                config_changed = true;
            }
            active.proxy_type = Set(proxy_type.clone());
            changed = true;
        }
        if let Some(ref local_ip) = req.local_ip {
            if local_ip != &proxy.local_ip {
                config_changed = true;
            }
            active.local_ip = Set(local_ip.clone());
            changed = true;
        }
        if let Some(local_port) = req.local_port {
            if local_port != proxy.local_port {
                config_changed = true;
            }
            active.local_port = Set(local_port);
            changed = true;
        }

        if changed {
            active.updated_at = Set(now);
            if let Err(e) = active.update(db).await {
                tracing::error!("更新代理 {} 失败: {}", proxy.id, e);
            }
        }
    }

    // 如果配置变更且代理已启用，重启监听器
    if config_changed {
        for proxy in &proxies {
            if proxy.enabled {
                let _ = app_state.proxy_control.stop_proxy(&client_id, proxy.id).await;
                if let Err(e) = app_state.proxy_control.start_proxy(&client_id, proxy.id).await {
                    tracing::error!("重启代理监听器失败 (ID: {}): {}", proxy.id, e);
                }
            }
        }

        // 通知客户端
        let csm = app_state.client_stream_manager.clone();
        let client_id_notify = client_id.clone();
        tokio::spawn(async move {
            csm.notify_proxy_change(&client_id_notify).await;
        });
    }

    info!("代理组 {} 已更新", group_id);
    (StatusCode::OK, ApiResponse::success("代理组更新成功"))
}
