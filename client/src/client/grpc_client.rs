//! Agent Client gRPC Client
//!
//! 连接 Controller 的 gRPC 双向流，处理认证、接收代理列表推送。

use anyhow::{anyhow, Result};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tonic::transport::{Channel, ClientTlsConfig};
use tracing::{error, info, warn, debug};

use common::config::KcpConfig;
use common::grpc::oxiproxy;
use common::grpc::oxiproxy::agent_client_message::Payload as ClientPayload;
use common::grpc::oxiproxy::controller_to_client_message::Payload as ControllerPayload;
use common::grpc::AgentClientServiceClient;
use common::protocol::client_config::{
    ProxyInfo as ClientProxyInfo, ServerProxyGroup as ClientServerProxyGroup,
};
use common::TunnelProtocol;

use super::log_collector::LogCollector;

/// 连接 Controller 并认证，返回代理列表更新的接收器
pub async fn connect_and_run(
    controller_url: &str,
    token: &str,
    tls_ca_cert: Option<&[u8]>,
    log_collector: LogCollector,
) -> Result<(i64, String, mpsc::Receiver<Vec<ClientServerProxyGroup>>)> {
    let mut endpoint = Channel::from_shared(controller_url.to_string())?
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .keep_alive_timeout(Duration::from_secs(10));

    if controller_url.starts_with("https://") {
        // 从 URL 中提取域名用于 SNI
        let domain = controller_url
            .trim_start_matches("https://")
            .split(':')
            .next()
            .ok_or_else(|| anyhow!("无法从 URL 提取域名"))?;

        let mut tls_config = ClientTlsConfig::new()
            .domain_name(domain)
            .with_webpki_roots();

        if let Some(ca_pem) = tls_ca_cert {
            info!("使用自定义 CA 证书进行 TLS 验证");
            tls_config = tls_config.ca_certificate(
                tonic::transport::Certificate::from_pem(ca_pem)
            );
        }

        endpoint = endpoint.tls_config(tls_config)
            .map_err(|e| anyhow!("TLS 配置失败: {}", e))?;
    }

    let channel = endpoint.connect()
        .await
        .map_err(|e| anyhow!("连接 Controller gRPC 失败: {}", e))?;

    let mut client = AgentClientServiceClient::new(channel);

    // 创建双向流
    let (tx, rx) = mpsc::channel::<oxiproxy::AgentClientMessage>(64);
    let (update_tx, update_rx) = mpsc::channel::<Vec<ClientServerProxyGroup>>(16);

    // 发送认证请求作为首条消息
    let auth_msg = oxiproxy::AgentClientMessage {
        payload: Some(ClientPayload::Auth(oxiproxy::ClientAuthRequest {
            token: token.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        })),
    };
    tx.send(auth_msg)
        .await
        .map_err(|_| anyhow!("发送认证消息失败"))?;

    // 建立 gRPC 流
    let outbound = tokio_stream::wrappers::ReceiverStream::new(rx);
    let response = client
        .agent_client_channel(outbound)
        .await
        .map_err(|e| anyhow!("建立 gRPC 流失败: {}", e))?;

    let mut inbound = response.into_inner();

    // 读取认证响应
    let first_msg = inbound
        .next()
        .await
        .ok_or_else(|| anyhow!("未收到认证响应"))?
        .map_err(|e| anyhow!("读取认证响应失败: {}", e))?;

    let auth_resp = match first_msg.payload {
        Some(ControllerPayload::AuthResponse(resp)) => resp,
        Some(ControllerPayload::Error(err)) => {
            return Err(anyhow!("认证被拒绝: [{}] {}", err.code, err.message));
        }
        _ => return Err(anyhow!("首条响应不是认证响应")),
    };

    if !auth_resp.success {
        return Err(anyhow!(
            "认证失败: {}",
            auth_resp.error_message.unwrap_or_default()
        ));
    }

    let client_id = auth_resp.client_id;
    let client_name = auth_resp.client_name.clone();
    info!("客户端认证成功: {} (ID: {})", client_name, client_id);

    // 启动消息接收循环
    let response_tx = tx.clone();
    tokio::spawn(async move {
        message_loop(inbound, update_tx, response_tx, log_collector).await;
    });

    // 启动心跳
    let heartbeat_tx = tx.clone();
    tokio::spawn(async move {
        heartbeat_loop(heartbeat_tx).await;
    });

    Ok((client_id, client_name, update_rx))
}

/// 消息接收循环
async fn message_loop(
    mut inbound: tonic::Streaming<oxiproxy::ControllerToClientMessage>,
    update_tx: mpsc::Sender<Vec<ClientServerProxyGroup>>,
    response_tx: mpsc::Sender<oxiproxy::AgentClientMessage>,
    log_collector: LogCollector,
) {
    while let Some(result) = inbound.next().await {
        let msg = match result {
            Ok(m) => m,
            Err(e) => {
                error!("gRPC 流错误: {}", e);
                break;
            }
        };

        let payload = match msg.payload {
            Some(p) => p,
            None => continue,
        };

        match payload {
            ControllerPayload::HeartbeatResponse(_) => {
                // 心跳响应，忽略
            }

            ControllerPayload::ProxyUpdate(update) => {
                debug!("收到代理配置更新: {} 个节点", update.server_groups.len());
                let groups = convert_server_groups(update.server_groups);
                if update_tx.send(groups).await.is_err() {
                    warn!("代理列表更新通道已关闭");
                    break;
                }
            }

            ControllerPayload::Error(err) => {
                error!("收到 Controller 错误通知: [{}] {}", err.code, err.message);
            }

            ControllerPayload::GetLogs(cmd) => {
                debug!("收到日志请求: count={}", cmd.count);
                let count = cmd.count as usize;
                let logs = if count == 0 {
                    log_collector.get_all_logs()
                } else {
                    log_collector.get_recent_logs(count)
                };

                let grpc_logs: Vec<oxiproxy::LogEntry> = logs.into_iter().map(|l| oxiproxy::LogEntry {
                    timestamp: l.timestamp.to_rfc3339(),
                    level: l.level,
                    message: l.message,
                }).collect();

                let resp_msg = oxiproxy::AgentClientMessage {
                    payload: Some(ClientPayload::Response(oxiproxy::AgentClientResponse {
                        request_id: cmd.request_id,
                        result: Some(oxiproxy::agent_client_response::Result::ClientLogs(
                            oxiproxy::ClientLogsResponse { logs: grpc_logs },
                        )),
                    })),
                };

                if response_tx.send(resp_msg).await.is_err() {
                    warn!("发送日志响应失败，连接可能已断开");
                    break;
                }
            }

            ControllerPayload::SoftwareUpdate(cmd) => {
                info!("收到远程软件更新指令，开始更新...");
                let update_result = tokio::task::spawn_blocking(perform_client_self_update).await;
                let (success, error_msg, new_ver) = match update_result {
                    Ok(Ok(v)) => (true, None, Some(v)),
                    Ok(Err(e)) => (false, Some(e.to_string()), None),
                    Err(e) => (false, Some(e.to_string()), None),
                };

                let resp_msg = oxiproxy::AgentClientMessage {
                    payload: Some(ClientPayload::Response(oxiproxy::AgentClientResponse {
                        request_id: cmd.request_id,
                        result: Some(oxiproxy::agent_client_response::Result::SoftwareUpdate(
                            oxiproxy::SoftwareUpdateResponse {
                                success,
                                error: error_msg,
                                new_version: new_ver,
                            },
                        )),
                    })),
                };

                let _ = response_tx.send(resp_msg).await;
                if success {
                    info!("软件更新成功，3秒后重启...");
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    restart_self();
                }
            }

            _ => {
                warn!("收到未知的 Controller 消息类型");
            }
        }
    }

    warn!("gRPC 连接断开");
}

/// 心跳循环
async fn heartbeat_loop(sender: mpsc::Sender<oxiproxy::AgentClientMessage>) {
    let mut interval = tokio::time::interval(Duration::from_secs(15));
    interval.tick().await; // 跳过首次

    loop {
        interval.tick().await;

        let msg = oxiproxy::AgentClientMessage {
            payload: Some(ClientPayload::Heartbeat(oxiproxy::Heartbeat {
                timestamp: chrono::Utc::now().timestamp(),
            })),
        };

        if sender.send(msg).await.is_err() {
            warn!("心跳发送失败，连接可能已断开");
            break;
        }
    }
}

/// 将 gRPC ServerProxyGroup 转换为 client_config::ServerProxyGroup
fn convert_server_groups(
    grpc_groups: Vec<oxiproxy::ServerProxyGroup>,
) -> Vec<ClientServerProxyGroup> {
    grpc_groups
        .into_iter()
        .map(|g| {
            let protocol = match g.protocol.as_str() {
                "kcp" => TunnelProtocol::Kcp,
                "tcp" => TunnelProtocol::Tcp,
                _ => TunnelProtocol::Quic,
            };

            let kcp = g.kcp.map(|k| KcpConfig {
                nodelay: k.nodelay,
                interval: k.interval,
                resend: k.resend,
                nc: k.nc,
            });

            let proxies = g
                .proxies
                .into_iter()
                .map(|p| ClientProxyInfo {
                    proxy_id: p.proxy_id,
                    name: p.name,
                    proxy_type: p.proxy_type,
                    local_ip: p.local_ip,
                    local_port: p.local_port,
                    remote_port: p.remote_port,
                    enabled: p.enabled,
                })
                .collect();

            ClientServerProxyGroup {
                node_id: g.node_id,
                server_addr: g.server_addr,
                server_port: g.server_port as u16,
                protocol,
                kcp,
                proxies,
            }
        })
        .collect()
}

/// 重启当前进程（用于自更新后重启）
///
/// - Unix: 使用 exec 替换当前进程（保持 PID 不变，适用于 daemon 模式）
/// - Windows: 启动新的分离进程后退出
fn restart_self() -> ! {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            error!("无法获取当前可执行文件路径: {}", e);
            std::process::exit(1);
        }
    };

    let original_args: Vec<String> = std::env::args().collect();

    // 构建重启参数：将 "daemon" 替换为 "start"，过滤掉 daemon 特有的 --pid-file 参数
    let mut restart_args: Vec<String> = Vec::new();
    let mut skip_next = false;

    for (i, arg) in original_args.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }

        // 第一个参数是子命令，daemon -> start（已在 daemon 上下文中，无需再次 fork）
        if i == 1 && arg == "daemon" {
            restart_args.push("start".to_string());
            continue;
        }

        // 跳过 daemon 特有的 --pid-file 参数及其值
        if arg == "--pid-file" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("--pid-file=") {
            continue;
        }

        restart_args.push(arg.clone());
    }

    info!("重启进程: {:?} {:?}", exe, restart_args);

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec 替换当前进程，保持 PID 不变
        let err = std::process::Command::new(&exe).args(&restart_args).exec();
        error!("exec 重启失败: {}", err);
        std::process::exit(1);
    }

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x00000008;
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        match std::process::Command::new(&exe)
            .args(&restart_args)
            .creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW)
            .spawn()
        {
            Ok(child) => info!("新进程已启动 (PID: {})", child.id()),
            Err(e) => error!("重启失败: {}", e),
        }
        std::process::exit(0);
    }
}

/// 执行客户端自更新（阻塞操作，需在 spawn_blocking 中调用）
fn perform_client_self_update() -> anyhow::Result<String> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("oxiproxy")
        .repo_name("oxiproxy")
        .bin_name("client")
        .identifier("client")
        .bin_path_in_archive("{bin}{bin_ext}")
        .show_download_progress(false)
        .current_version(env!("CARGO_PKG_VERSION"))
        .no_confirm(true)
        .build()?
        .update()?;

    match status {
        self_update::Status::UpToDate(v) => Ok(v),
        self_update::Status::Updated(v) => Ok(v),
    }
}
