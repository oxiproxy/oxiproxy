pub mod proxy_server;
pub mod traffic;
pub mod client_logs;
pub mod config_manager;
pub mod local_proxy_control;
pub mod grpc_client;
pub mod grpc_auth_provider;
pub mod node_logs;
pub mod tunnel_manager;
pub mod speed_limiter;

use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, error, warn};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use common::protocol::control::ProxyControl;
use common::protocol::auth::ClientAuthProvider;

/// Agent Server 启动（Controller 模式，gRPC）
///
/// 通过 gRPC 双向流连接 Controller，支持断线自动重连。
pub async fn run_server_controller_mode(
    controller_url: String,
    token: String,
    protocol: String,
    tls_ca_cert: Option<Vec<u8>>,
    log_dir: Option<String>,
) -> Result<()> {
    // 初始化内存日志缓冲区（保存最近 1000 条日志）
    let log_buffer = node_logs::init_global_log_buffer(1000);
    let log_layer = node_logs::NodeLogLayer::new(log_buffer);

    // 初始化 tracing 日志系统
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx::query=warn"));

    // 按天轮转文件日志（daemon 模式）或控制台日志（前台模式）
    if let Some(dir) = &log_dir {
        let file_appender = tracing_appender::rolling::daily(dir, "node.log");
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_writer(file_appender).with_ansi(false))
            .with(log_layer)
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer())
            .with(log_layer)
            .init();
    }

    info!("Agent Server 启动 (Controller gRPC 模式)");
    info!("Controller: {}", controller_url);
    info!("隧道协议: {}", protocol);

    // 首次连接 Controller 并认证（protocol 作为回退值，最终以 Controller 返回为准）
    let (grpc_client, cmd_rx, authoritative_protocol, initial_speed_limit, bind_port, node_certificate) = grpc_client::AgentGrpcClient::connect_and_authenticate(
        &controller_url,
        &token,
        &protocol,
        tls_ca_cert.as_deref(),
    ).await?;

    let node_id = grpc_client.node_id().await;
    info!("连接认证成功: 节点 #{}, Controller 协议: {}, 隧道端口: {}", node_id, authoritative_protocol, bind_port);

    // 提取证书 PEM
    let (cert_pem, key_pem) = if let Some(cert) = node_certificate {
        (Some(cert.cert_pem), Some(cert.key_pem))
    } else {
        (None, None)
    };

    // 创建速度限制器（0 表示不限速）
    let speed_limiter = speed_limiter::SpeedLimiter::new(initial_speed_limit.unwrap_or(0) as u64);
    if let Some(limit) = initial_speed_limit {
        if limit > 0 {
            info!("速度限制: {} bytes/sec", limit);
        }
    }

    // 创建 gRPC 认证提供者（使用 SharedGrpcSender，重连后自动使用新 sender）
    let auth_provider: Arc<dyn ClientAuthProvider> = Arc::new(
        grpc_auth_provider::GrpcAuthProvider::new(&grpc_client, node_id)
    );

    // 创建 gRPC 流量管理器（使用 SharedGrpcSender，重连后自动使用新 sender）
    let traffic_manager = Arc::new(
        traffic::TrafficManager::new(grpc_client.shared_sender().clone())
    );

    // 创建配置管理器（使用默认值）
    let config_manager = Arc::new(config_manager::ConfigManager::new());

    // 创建 ProxyServer
    let proxy_server = Arc::new(
        proxy_server::ProxyServer::new(
            traffic_manager.clone(),
            config_manager.clone(),
            auth_provider.clone(),
            speed_limiter.clone(),
            cert_pem,
            key_pem,
        )?
    );

    // 创建本地代理控制实例
    let proxy_control: Arc<dyn ProxyControl> = Arc::new(local_proxy_control::LocalProxyControl::new(
        proxy_server.get_listener_manager(),
        proxy_server.get_client_connections(),
        proxy_server.get_tunnel_connections(),
        auth_provider.clone(),
        proxy_server.clone(),
    ));

    // 创建并启动隧道管理器（使用 Controller 下发的权威协议）
    let tunnel_manager = Arc::new(tunnel_manager::TunnelManager::new(proxy_server.clone(), bind_port));
    tunnel_manager.start(&authoritative_protocol, None).await?;

    // 启动首次 Controller 命令处理器
    let grpc_client_clone = grpc_client.clone();
    let proxy_control_clone = proxy_control.clone();
    let tunnel_manager_clone = tunnel_manager.clone();
    let speed_limiter_clone = speed_limiter.clone();
    tokio::spawn(async move {
        grpc_client::handle_controller_commands(cmd_rx, grpc_client_clone, proxy_control_clone, tunnel_manager_clone, speed_limiter_clone).await;
    });

    info!("所有服务已启动");

    // gRPC 断线重连监控循环
    let grpc_client_reconnect = grpc_client.clone();
    let proxy_control_reconnect = proxy_control.clone();
    let tunnel_manager_reconnect = tunnel_manager.clone();
    let speed_limiter_reconnect = speed_limiter.clone();
    let controller_url_clone = controller_url.clone();
    let token_clone = token.clone();
    let protocol_clone = protocol.clone();

    let tls_ca_cert_clone = tls_ca_cert.clone();

    tokio::spawn(async move {
        // 等待首次连接的心跳/消息循环结束（通过检测 sender 是否可用）
        // 使用简单的轮询检测连接状态
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;

            // 尝试发送一个心跳来检测连接是否存活
            let test_msg = common::grpc::oxiproxy::AgentServerMessage {
                payload: Some(common::grpc::oxiproxy::agent_server_message::Payload::Heartbeat(
                    common::grpc::oxiproxy::Heartbeat {
                        timestamp: chrono::Utc::now().timestamp(),
                    },
                )),
            };

            if grpc_client_reconnect.shared_sender().send(test_msg).await.is_err() {
                warn!("检测到 gRPC 连接断开，开始重连...");

                loop {
                    match grpc_client_reconnect.reconnect(
                        &controller_url_clone,
                        &token_clone,
                        &protocol_clone,
                        tls_ca_cert_clone.as_deref(),
                    ).await {
                        Ok((new_cmd_rx, new_protocol, new_speed_limit, new_tunnel_port, new_certificate)) => {
                            info!("gRPC 重连成功");

                            // 如果收到新证书，记录日志（暂不支持热更新证书，需要重启隧道）
                            if let Some(cert) = new_certificate {
                                info!("⚠️  收到新证书（指纹: {}），需要重启 Node 以应用", cert.fingerprint);
                            }

                            // 更新速度限制
                            if let Some(limit) = new_speed_limit {
                                speed_limiter_reconnect.update_rate(limit as u64);
                            }

                            // 如果协议或端口变更，重启隧道
                            if !new_protocol.is_empty() {
                                tunnel_manager_reconnect.update_port(new_tunnel_port);
                                if let Err(e) = tunnel_manager_reconnect.switch_protocol(&new_protocol).await {
                                    error!("重连后切换协议失败: {}", e);
                                }
                            }

                            // 启动新的命令处理器
                            let grpc_clone = grpc_client_reconnect.clone();
                            let control_clone = proxy_control_reconnect.clone();
                            let tm_clone = tunnel_manager_reconnect.clone();
                            let sl_clone = speed_limiter_reconnect.clone();
                            tokio::spawn(async move {
                                grpc_client::handle_controller_commands(
                                    new_cmd_rx, grpc_clone, control_clone, tm_clone, sl_clone,
                                ).await;
                            });

                            break;
                        }
                        Err(e) => {
                            error!("gRPC 重连失败: {}", e);
                            warn!("5秒后重试...");
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }
    });

    // 等待终止信号
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("收到 Ctrl+C 信号，正在关闭服务...");
        }
        _ = async {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate()).expect("failed to listen for SIGTERM");
                sigterm.recv().await;
            }
            #[cfg(not(unix))]
            {
                std::future::pending::<()>().await;
            }
        } => {
            info!("收到 SIGTERM 信号，正在关闭服务...");
        }
    }

    Ok(())
}
