pub mod connector;
pub mod log_collector;
pub mod connection_manager;
pub mod grpc_client;

use anyhow::Result;
use common::utils::ReconnectBackoff;
use tracing::{info, error, warn};
use tracing_subscriber::{EnvFilter, fmt, prelude::*, layer::SubscriberExt};
use log_collector::{LogCollector, LogCollectorLayer};

pub async fn run_client(
    controller_url: String,
    token: String,
    tls_ca_cert: Option<Vec<u8>>,
    log_dir: Option<String>,
) -> Result<()> {
    // 初始化日志收集器（保留最近 1000 条日志）
    let log_collector = LogCollector::new(1000);

    // 初始化 tracing 日志系统
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx::query=warn"));

    // 按天轮转文件日志（daemon 模式）或控制台日志（前台模式）
    if let Some(dir) = &log_dir {
        let file_appender = tracing_appender::rolling::daily(dir, "client.log");
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer().with_writer(file_appender).with_ansi(false))
            .with(LogCollectorLayer::new(log_collector.clone()))
            .init();
    } else {
        tracing_subscriber::registry()
            .with(env_filter)
            .with(fmt::layer())
            .with(LogCollectorLayer::new(log_collector.clone()))
            .init();
    }

    info!("OxiProxy 客户端启动");
    info!("控制器地址: {}", controller_url);

    // Controller 模式：通过 gRPC 双向流接收代理列表推送
    let conn_manager = connection_manager::ConnectionManager::new(
        token.clone(),
        log_collector.clone(),
    );

    // 断线重连循环（指数退避 + 抖动，避免 Controller 宕机时的重试风暴）
    let mut backoff = ReconnectBackoff::default_params();
    loop {
        match grpc_client::connect_and_run(&controller_url, &token, tls_ca_cert.as_deref(), log_collector.clone()).await {
            Ok((_client_id, client_name, mut update_rx)) => {
                info!("已连接控制器: {}", client_name);
                // 连接成功后重置退避
                backoff.reset();

                // 接收代理列表推送并调和连接
                while let Some(server_groups) = update_rx.recv().await {
                    info!("代理配置已更新: {} 个节点", server_groups.len());
                    conn_manager.reconcile(server_groups).await;
                }

                warn!("控制器连接断开");
            }
            Err(e) => {
                error!("连接控制器失败: {}", e);
            }
        }

        let delay = backoff.next_delay();
        warn!("{:.1}s 后重连...", delay.as_secs_f32());
        tokio::time::sleep(delay).await;
    }
}
