use anyhow::Result;
use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, error, warn, debug};
use crate::client::log_collector::LogCollector;

// 从共享库导入隧道模块
use common::{TunnelConnection, TunnelConnector, TunnelRecvStream, TunnelSendStream};
use common::utils::create_configured_udp_socket;

// Heartbeat configuration
const HEARTBEAT_INTERVAL_SECS: u64 = 10;
const HEARTBEAT_TIMEOUT_SECS: u64 = 15;

/// 单次连接尝试（供 controller 模式使用，不含重试循环）
pub async fn connect_once(
    connector: Arc<dyn TunnelConnector>,
    server_addr: SocketAddr,
    token: &str,
    log_collector: LogCollector,
) -> Result<()> {
    info!("连接节点: {}", server_addr);
    connect_to_server(connector, server_addr, token, log_collector).await
}

async fn connect_to_server(
    connector: Arc<dyn TunnelConnector>,
    server_addr: SocketAddr,
    token: &str,
    log_collector: LogCollector,
) -> Result<()> {
    // Connect to server
    let conn = connector.connect(server_addr).await?;
    let conn = Arc::new(conn);

    // Send token for authentication
    debug!("发送认证令牌");
    let mut uni_stream = conn.open_uni().await?;
    let token_bytes = token.as_bytes();
    let len = token_bytes.len() as u16;
    uni_stream.write_all(&len.to_be_bytes()).await?;
    uni_stream.write_all(token_bytes).await?;
    uni_stream.finish().await?;

    info!("节点认证成功: {}", server_addr);

    // Start application-level heartbeat task
    let conn_heartbeat = conn.clone();
    let heartbeat_failed = Arc::new(AtomicBool::new(false));
    let heartbeat_failed_clone = heartbeat_failed.clone();

    let mut heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(HEARTBEAT_INTERVAL_SECS));
        let mut consecutive_failures = 0u32;
        const MAX_FAILURES: u32 = 3;

        loop {
            interval.tick().await;

            // Check if connection is still valid
            if conn_heartbeat.close_reason().is_some() {
                warn!("检测到连接已关闭");
                heartbeat_failed_clone.store(true, Ordering::SeqCst);
                break;
            }

            // Send application-level heartbeat
            match send_heartbeat(&conn_heartbeat).await {
                Ok(_) => {
                    consecutive_failures = 0;
                    debug!("Heartbeat sent successfully");
                }
                Err(e) => {
                    consecutive_failures += 1;
                    warn!("心跳失败 ({}/{}): {}", consecutive_failures, MAX_FAILURES, e);

                    if consecutive_failures >= MAX_FAILURES {
                        error!("心跳连续失败 {} 次，连接已断开", MAX_FAILURES);
                        heartbeat_failed_clone.store(true, Ordering::SeqCst);
                        break;
                    }
                }
            }
        }
    });

    // Loop to accept streams from server
    loop {
        // Check if heartbeat failed
        if heartbeat_failed.load(Ordering::SeqCst) {
            error!("心跳检查失败，准备重连");
            return Err(anyhow::anyhow!("心跳失败"));
        }

        tokio::select! {
            // Monitor heartbeat task
            _ = &mut heartbeat_handle => {
                error!("心跳任务结束，准备重连");
                return Err(anyhow::anyhow!("心跳任务结束"));
            }
            // Accept new streams
            result = conn.accept_bi() => {
                match result {
                    Ok((quic_send, mut quic_recv)) => {
                        let collector = log_collector.clone();

                        tokio::spawn(async move {
                            // Read message type (1 byte)
                            let mut msg_type_buf = [0u8; 1];
                            if quic_recv.read_exact(&mut msg_type_buf).await.is_err() {
                                return;
                            }

                            match msg_type_buf[0] {
                                b'p' => {
                                    // 'p' = proxy request
                                    debug!("收到代理请求");
                                    if let Err(e) = handle_proxy_stream(quic_send, quic_recv).await {
                                        error!("代理流处理错误: {}", e);
                                    }
                                }
                                b'l' => {
                                    // 'l' = log request
                                    debug!("收到日志请求");
                                    if let Err(e) = handle_log_request(quic_send, quic_recv, collector).await {
                                        error!("日志请求处理错误: {}", e);
                                    }
                                }
                                _ => {
                                    warn!("未知消息类型: {}", msg_type_buf[0]);
                                }
                            }
                        });
                    }
                    Err(e) => {
                        error!("接受流失败: {}", e);
                        return Err(e);
                    }
                }
            }
        }
    }
}

async fn handle_proxy_stream(
    quic_send: Box<dyn TunnelSendStream>,
    mut quic_recv: Box<dyn TunnelRecvStream>,
) -> Result<()> {
    // Read protocol type (1 byte)
    let mut proto_buf = [0u8; 1];
    quic_recv.read_exact(&mut proto_buf).await?;
    let protocol_type = proto_buf[0];

    // Read target address (format: 2 byte length + content)
    let mut len_buf = [0u8; 2];
    quic_recv.read_exact(&mut len_buf).await?;
    let len = u16::from_be_bytes(len_buf) as usize;

    let mut addr_buf = vec![0u8; len];
    quic_recv.read_exact(&mut addr_buf).await?;
    let target_addr = String::from_utf8(addr_buf)?;

    debug!("目标地址: {}, 协议: {}", target_addr,
          if protocol_type == b'u' { "UDP" } else { "TCP" });

    // Connect to target service based on protocol type
    match protocol_type {
        b't' => {
            // TCP connection
            handle_tcp_proxy(quic_send, quic_recv, &target_addr).await?;
        }
        b'u' => {
            // UDP connection
            handle_udp_proxy(quic_send, quic_recv, &target_addr).await?;
        }
        _ => {
            error!("未知协议类型: {}", protocol_type);
            return Err(anyhow::anyhow!("未知协议类型: {}", protocol_type));
        }
    }

    Ok(())
}

async fn handle_tcp_proxy(
    mut quic_send: Box<dyn TunnelSendStream>,
    mut quic_recv: Box<dyn TunnelRecvStream>,
    target_addr: &str,
) -> Result<()> {
    // Connect to target service
    let mut tcp_stream = TcpStream::connect(target_addr).await?;

    debug!("已连接目标服务: {}", target_addr);

    let (mut tcp_read, mut tcp_write) = tcp_stream.split();

    // Tunnel -> TCP
    let tunnel_to_tcp = async {
        let mut buf = vec![0u8; 32768];
        loop {
            match quic_recv.read(&mut buf).await? {
                Some(n) => {
                    if n == 0 {
                        break;
                    }
                    tcp_write.write_all(&buf[..n]).await?;
                }
                None => break,
            }
        }
        let _ = tcp_write.shutdown().await;
        Ok::<_, anyhow::Error>(())
    };

    // TCP -> Tunnel
    let tcp_to_tunnel = async {
        let mut buf = vec![0u8; 32768];
        loop {
            let n = tcp_read.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            quic_send.write_all(&buf[..n]).await?;
        }
        let _ = quic_send.finish().await;
        Ok::<_, anyhow::Error>(())
    };

    let (res1, res2) = tokio::join!(tunnel_to_tcp, tcp_to_tunnel);
    if let Err(e) = res1 {
        debug!("Tunnel->TCP 传输结束: {}", e);
    }
    if let Err(e) = res2 {
        debug!("TCP->Tunnel 传输结束: {}", e);
    }

    Ok(())
}

async fn handle_udp_proxy(
    mut quic_send: Box<dyn TunnelSendStream>,
    mut quic_recv: Box<dyn TunnelRecvStream>,
    target_addr: &str,
) -> Result<()> {
    // Bind a UDP socket
    let socket = create_configured_udp_socket("0.0.0.0:0".parse()?).await?;
    debug!("UDP 代理已启动: {}", target_addr);

    // Read initial UDP data from server
    let mut recv_buf = vec![0u8; 65535];
    let initial_len = match quic_recv.read(&mut recv_buf).await? {
        Some(n) => n,
        None => {
            debug!("未收到初始 UDP 数据");
            return Ok(());
        }
    };

    // Send data to target address
    socket.send_to(&recv_buf[..initial_len], target_addr).await?;
    debug!("Sent {} bytes UDP data to {}", initial_len, target_addr);

    // Set TTL
    socket.set_ttl(64)?;

    // Loop to receive responses from target and forward back to server
    loop {
        let mut response_buf = vec![0u8; 65535];
        tokio::select! {
            // Read data from QUIC (more UDP packets from server)
            result = quic_recv.read(&mut recv_buf) => {
                match result? {
                    Some(n) => {
                        if n > 0 {
                            // Forward to target
                            socket.send_to(&recv_buf[..n], target_addr).await?;
                            debug!("Forwarded UDP packet: {} bytes", n);
                        } else {
                            break;
                        }
                    }
                    None => break,
                }
            }
            // Read UDP response from target
            result = socket.recv_from(&mut response_buf) => {
                match result {
                    Ok((len, _from)) => {
                        // Send back to server
                        quic_send.write_all(&response_buf[..len]).await?;
                    }
                    Err(e) => {
                        error!("UDP 接收错误: {}", e);
                        break;
                    }
                }
            }
        }
    }

    // Close QUIC stream
    quic_send.finish().await?;

    Ok(())
}

/// Send application-level heartbeat
/// Heartbeat protocol: client sends 'h' (heartbeat), server replies 'h'
async fn send_heartbeat(conn: &Arc<Box<dyn TunnelConnection>>) -> Result<()> {
    // Open bidirectional stream for heartbeat
    let (mut send, mut recv) = tokio::time::timeout(
        Duration::from_secs(HEARTBEAT_TIMEOUT_SECS),
        conn.open_bi()
    ).await.map_err(|_| anyhow::anyhow!("Heartbeat open_bi timeout"))??;

    // Send heartbeat request 'h'
    send.write_all(&[b'h']).await?;
    send.flush().await?;

    // Wait for server reply
    let mut response = [0u8; 1];
    tokio::time::timeout(
        Duration::from_secs(HEARTBEAT_TIMEOUT_SECS),
        recv.read_exact(&mut response)
    ).await.map_err(|_| anyhow::anyhow!("Heartbeat response timeout"))??;

    if response[0] != b'h' {
        return Err(anyhow::anyhow!("Invalid heartbeat response: {}", response[0]));
    }

    // Close stream
    send.finish().await?;

    Ok(())
}

/// Handle log request
async fn handle_log_request(
    mut quic_send: Box<dyn TunnelSendStream>,
    mut quic_recv: Box<dyn TunnelRecvStream>,
    log_collector: LogCollector,
) -> Result<()> {
    // Read requested log count (2 bytes)
    let mut count_buf = [0u8; 2];
    quic_recv.read_exact(&mut count_buf).await?;
    let count = u16::from_be_bytes(count_buf) as usize;

    debug!("Requested log count: {}", count);

    // Get logs
    let logs = if count == 0 {
        log_collector.get_all_logs()
    } else {
        log_collector.get_recent_logs(count)
    };

    // Serialize logs to JSON
    let logs_json = serde_json::to_string(&logs)?;
    let logs_bytes = logs_json.as_bytes();

    // Send log data length (4 bytes)
    let len = logs_bytes.len() as u32;
    quic_send.write_all(&len.to_be_bytes()).await?;

    // Send log data
    quic_send.write_all(logs_bytes).await?;
    quic_send.finish().await?;

    debug!("已发送 {} 条日志 ({} 字节)", logs.len(), logs_bytes.len());

    Ok(())
}
