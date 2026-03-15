use axum::middleware::from_fn;
use axum::{Extension, Router};
use axum::routing::{get, post, put, delete};
use axum::response::IntoResponse;
use axum::http::{header, StatusCode};
use tower_http::cors::CorsLayer;
use tracing::{info, error, warn};
use crate::AppState;
use crate::middleware::auth_middleware;
use std::sync::Arc;
use axum_server::tls_rustls::RustlsConfig;
use axum_server_dual_protocol::ServerExt;
use base64::Engine;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../dist"]
struct Assets;

pub mod handlers;

/// 嵌入式静态文件服务 handler（SPA fallback）
async fn static_handler(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');

    // 尝试匹配请求路径的文件
    if !path.is_empty() {
        if let Some(file) = Assets::get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.to_vec(),
            ).into_response();
        }
    }

    // SPA fallback: 返回 index.html
    match Assets::get("index.html") {
        Some(file) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html".to_string())],
            file.data.to_vec(),
        ).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            [(header::CONTENT_TYPE, "text/plain".to_string())],
            "Dashboard not found. Build the frontend first: cd dashboard && bun run build".as_bytes().to_vec(),
        ).into_response(),
    }
}

/// 从 ConfigManager 加载 Web TLS 证书和私钥
async fn load_web_tls_config(config_manager: &crate::config_manager::ConfigManager) -> Option<RustlsConfig> {
    let tls_enabled = config_manager.get_bool("web_tls_enabled", false).await;
    if !tls_enabled {
        return None;
    }

    // 优先从数据库内容读取（base64 编码的 PEM）
    let cert_content = config_manager.get_string("web_tls_cert_content", "").await;
    let key_content = config_manager.get_string("web_tls_key_content", "").await;

    if !cert_content.is_empty() && !key_content.is_empty() {
        match (
            base64::engine::general_purpose::STANDARD.decode(&cert_content),
            base64::engine::general_purpose::STANDARD.decode(&key_content),
        ) {
            (Ok(cert_pem), Ok(key_pem)) => {
                match RustlsConfig::from_pem(cert_pem, key_pem).await {
                    Ok(config) => {
                        info!("从数据库加载 Web TLS 证书");
                        return Some(config);
                    }
                    Err(e) => {
                        error!("Web TLS 证书加载失败: {}", e);
                    }
                }
            }
            _ => {
                error!("Web TLS 证书 base64 解码失败");
            }
        }
    }

    // 回退到文件路径
    let cert_path = config_manager.get_string("web_tls_cert_path", "").await;
    let key_path = config_manager.get_string("web_tls_key_path", "").await;

    if !cert_path.is_empty() && !key_path.is_empty() {
        match RustlsConfig::from_pem_file(&cert_path, &key_path).await {
            Ok(config) => {
                info!("从文件加载 Web TLS 证书: {}", cert_path);
                return Some(config);
            }
            Err(e) => {
                error!("从文件加载 Web TLS 证书失败: {}", e);
            }
        }
    }

    warn!("Web TLS 已启用但未配置有效证书，回退到 HTTP 模式");
    None
}

/// 启动 Web API 服务
pub fn start_web_server(app_state: AppState) -> tokio::task::JoinHandle<()> {
    let web_port = app_state.config.web_port;
    let config_manager = app_state.config_manager.clone();

    tokio::spawn(async move {
        // 构建 Web 应用
        let api_routes = Router::new()
            // 公开路由（无需认证）
            .route("/auth/login", post(handlers::login))
            .route("/auth/register", post(handlers::register))
            .route("/auth/register-status", get(handlers::get_register_status))
            .route("/client/connect-config", post(handlers::get_client_connect_config))
            // 认证路由（需要登录）
            .route("/auth/me", get(handlers::me))
            // 仪表板路由
            .route("/dashboard/stats/{user_id}", get(handlers::get_user_dashboard_stats))
            .route("/clients", get(handlers::list_clients).post(handlers::create_client))
            .route("/clients/batch-update", post(handlers::batch_update_clients))
            .route("/clients/{id}", get(handlers::get_client).delete(handlers::delete_client))
            .route("/clients/{id}/logs", get(handlers::get_client_logs))
            .route("/clients/{id}/traffic", get(handlers::get_client_traffic))
            .route("/clients/{id}/allocate-quota", post(handlers::allocate_client_quota))
            .route("/clients/{id}/update", post(handlers::trigger_client_update))
            .route("/proxies", get(handlers::list_proxies).post(handlers::create_proxy))
            .route("/proxies/batch", post(handlers::batch_create_proxies))
            .route("/proxies/group/{group_id}", put(handlers::update_proxy_group).delete(handlers::delete_proxy_group))
            .route("/proxies/group/{group_id}/toggle", post(handlers::toggle_proxy_group))
            .route("/proxies/{id}", put(handlers::update_proxy).delete(handlers::delete_proxy))
            .route("/clients/{id}/proxies", get(handlers::list_proxies_by_client))
            // 流量统计路由
            .route("/traffic/overview", get(handlers::get_traffic_overview_handler))
            .route("/traffic/users/{id}", get(handlers::get_user_traffic_handler))
            // 系统配置路由
            .route("/system/configs", get(handlers::get_configs))
            .route("/system/configs/update", post(handlers::update_config))
            .route("/system/configs/batch", post(handlers::batch_update_configs))
            .route("/system/restart", post(handlers::restart_system))
            .route("/system/latest-version", get(handlers::get_latest_version))
            // 管理员路由（需要管理员权限）
            .route("/users", get(handlers::list_users).post(handlers::create_user))
            .route("/users/{id}", put(handlers::update_user).delete(handlers::delete_user))
            .route("/users/{id}/nodes", get(handlers::get_user_nodes))
            .route("/users/{id}/nodes/{node_id}", post(handlers::assign_node_to_user).delete(handlers::remove_node_from_user))
            .route("/users/{id}/adjust-quota", post(handlers::adjust_user_quota))
            .route("/users/{id}/quota-info", get(handlers::get_user_quota_info))
            // 节点管理路由（管理员权限）
            .route("/nodes", get(handlers::list_nodes).post(handlers::create_node))
            .route("/nodes/batch-update", post(handlers::batch_update_nodes))
            .route("/nodes/{id}", get(handlers::get_node).put(handlers::update_node).delete(handlers::delete_node))
            .route("/nodes/{id}/test", post(handlers::test_node_connection))
            .route("/nodes/{id}/status", get(handlers::get_node_status))
            .route("/nodes/{id}/logs", get(handlers::get_node_logs))
            .route("/nodes/{id}/update", post(handlers::trigger_node_update))
            // 订阅管理路由
            .route("/subscriptions", get(handlers::list_subscriptions).post(handlers::create_subscription))
            .route("/subscriptions/active", get(handlers::list_active_subscriptions))
            .route("/subscriptions/{id}", get(handlers::get_subscription).put(handlers::update_subscription).delete(handlers::delete_subscription))
            // 用户订阅路由
            .route("/user-subscriptions", get(handlers::list_user_subscriptions).post(handlers::create_user_subscription))
            .route("/user-subscriptions/{id}", put(handlers::update_user_subscription).delete(handlers::delete_user_subscription))
            .route("/users/{user_id}/subscriptions", get(handlers::get_user_subscriptions))
            .route("/users/{user_id}/subscriptions/active", get(handlers::get_user_active_subscription))
            // 应用认证中间件
            .layer(from_fn(auth_middleware))
            // 添加应用状态
            .layer(Extension(app_state));

        let app = Router::new()
            // API 路由
            .nest("/api", api_routes)
            // 嵌入式静态文件服务，带 SPA fallback
            .fallback(static_handler)
            .layer(CorsLayer::permissive());

        let web_addr = format!("0.0.0.0:{}", web_port);

        // 尝试加载 TLS 配置
        if let Some(tls_config) = load_web_tls_config(&config_manager).await {
            // 读取是否开启 HTTP→HTTPS 自动跳转（反向代理场景下应关闭）
            let force_https = config_manager.get_bool("web_tls_force_https", true).await;
            info!("🌐 Web管理界面: https://{}", web_addr);
            if !force_https {
                info!("HTTP→HTTPS 自动跳转已关闭（适用于反向代理部署）");
            }
            match axum_server_dual_protocol::bind_dual_protocol(web_addr.parse().expect("invalid web listen address"), tls_config)
                .set_upgrade(force_https)
                .serve(app.into_make_service())
                .await
            {
                Ok(_) => {}
                Err(err) => {
                    error!("Web服务错误：{}", err);
                }
            }
        } else {
            // 使用 HTTP
            match tokio::net::TcpListener::bind(web_addr.clone()).await {
                Ok(listener) => {
                    info!("🌐 Web管理界面: http://{}", web_addr);
                    if let Err(err) = axum::serve(listener, app).await {
                        error!("Web服务错误：{}", err);
                    }
                }
                Err(err) => {
                    error!("Web服务启动失败：{}", err);
                }
            }
        }
    })
}
