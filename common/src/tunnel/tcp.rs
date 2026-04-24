//! TCP 协议隧道实现
//!
//! 此模块提供了基于 TCP 协议的隧道实现，使用 yamux 库进行多路复用：
//! - `TcpTunnelConnection`: 连接包装器（基于 yamux 多路复用）
//! - `TcpTunnelConnector`: 客户端连接器
//! - `TcpTunnelListener`: 服务端监听器
//!
//! 发送流和接收流复用与 KCP 相同的 yamux Stream 拆分模式。

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use futures::{AsyncReadExt, AsyncWriteExt};
use futures::io::{ReadHalf, WriteHalf};
use std::net::SocketAddr;
use std::task::Poll;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use tracing::{debug, warn};
use yamux::{Config as YamuxConfig, Connection as YamuxConnection, Mode, Stream as YamuxStream};

/// 应用标准 TCP 优化套接字选项（参考 rathole helper.rs:19-36）
/// - TCP_NODELAY: 禁用 Nagle，降低小包延迟
/// - SO_KEEPALIVE + TCP_KEEPIDLE/INTVL: 及时检测半开连接
fn apply_tcp_socket_opts(stream: &tokio::net::TcpStream) {
    // TCP_NODELAY（yamux 会频繁发送小帧，必须开启）
    if let Err(e) = stream.set_nodelay(true) {
        warn!("设置 TCP_NODELAY 失败: {}", e);
    }

    // keepalive: 60s 空闲后开始探测，每 10s 一次
    let sock_ref = socket2::SockRef::from(stream);
    let ka = socket2::TcpKeepalive::new()
        .with_time(Duration::from_secs(60))
        .with_interval(Duration::from_secs(10));
    if let Err(e) = sock_ref.set_tcp_keepalive(&ka) {
        warn!("设置 TCP keepalive 失败: {}", e);
    }
}

use super::traits::{TunnelConnection, TunnelConnector, TunnelListener, TunnelRecvStream, TunnelSendStream, TunnelStream};

/// TCP 发送流（基于 yamux Stream 写半流）
pub struct TcpTunnelSendStream {
    writer: Mutex<WriteHalf<YamuxStream>>,
}

impl TcpTunnelSendStream {
    fn new(writer: WriteHalf<YamuxStream>) -> Self {
        Self { writer: Mutex::new(writer) }
    }
}

#[async_trait]
impl TunnelSendStream for TcpTunnelSendStream {
    async fn write_all(&mut self, buf: &[u8]) -> Result<()> {
        let writer = self.writer.get_mut();
        writer.write_all(buf).await?;
        Ok(())
    }

    async fn flush(&mut self) -> Result<()> {
        let writer = self.writer.get_mut();
        writer.flush().await?;
        Ok(())
    }

    async fn finish(&mut self) -> Result<()> {
        let writer = self.writer.get_mut();
        writer.close().await?;
        Ok(())
    }
}

/// TCP 接收流（基于 yamux Stream 读半流）
pub struct TcpTunnelRecvStream {
    reader: Mutex<ReadHalf<YamuxStream>>,
}

impl TcpTunnelRecvStream {
    fn new(reader: ReadHalf<YamuxStream>) -> Self {
        Self { reader: Mutex::new(reader) }
    }
}

#[async_trait]
impl TunnelRecvStream for TcpTunnelRecvStream {
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()> {
        let reader = self.reader.get_mut();
        reader.read_exact(buf).await?;
        Ok(())
    }

    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>> {
        let reader = self.reader.get_mut();
        match reader.read(buf).await {
            Ok(0) => Ok(None),
            Ok(n) => Ok(Some(n)),
            Err(e) => Err(anyhow!("Read error: {}", e)),
        }
    }
}

/// Compat 包装器，将 tokio TcpStream 转换为 futures AsyncRead/AsyncWrite
type CompatTcpStream = tokio_util::compat::Compat<tokio::net::TcpStream>;

/// 出站流请求
struct OutboundRequest {
    response_tx: oneshot::Sender<Result<YamuxStream>>,
}

/// TCP 连接包装器
///
/// 使用 yamux 进行多路复用的 TCP 连接。
/// 通过后台驱动任务持续调用 yamux 的 poll_next_inbound 来驱动连接 I/O。
pub struct TcpTunnelConnection {
    inbound_rx: Mutex<mpsc::Receiver<YamuxStream>>,
    outbound_tx: mpsc::Sender<OutboundRequest>,
    close_reason_rx: watch::Receiver<Option<String>>,
    _driver_handle: tokio::task::JoinHandle<()>,
    remote_addr: SocketAddr,
}

impl TcpTunnelConnection {
    pub fn new(stream: tokio::net::TcpStream, remote_addr: SocketAddr, is_client: bool) -> Self {
        let compat_stream = stream.compat();
        let mode = if is_client { Mode::Client } else { Mode::Server };
        let config = YamuxConfig::default();
        let connection = YamuxConnection::new(compat_stream, config, mode);

        let (inbound_tx, inbound_rx) = mpsc::channel::<YamuxStream>(32);
        let (outbound_tx, outbound_rx) = mpsc::channel::<OutboundRequest>(32);
        let (close_reason_tx, close_reason_rx) = watch::channel(None);

        let driver_handle = tokio::spawn(run_tcp_yamux_driver(
            connection,
            inbound_tx,
            outbound_rx,
            close_reason_tx,
        ));

        Self {
            inbound_rx: Mutex::new(inbound_rx),
            outbound_tx,
            close_reason_rx,
            _driver_handle: driver_handle,
            remote_addr,
        }
    }
}

impl Drop for TcpTunnelConnection {
    fn drop(&mut self) {
        self._driver_handle.abort();
    }
}

/// TCP yamux 连接后台驱动任务
async fn run_tcp_yamux_driver(
    mut connection: YamuxConnection<CompatTcpStream>,
    inbound_tx: mpsc::Sender<YamuxStream>,
    mut outbound_rx: mpsc::Receiver<OutboundRequest>,
    close_reason_tx: watch::Sender<Option<String>>,
) {
    let mut pending_outbound: Vec<OutboundRequest> = Vec::new();

    let reason = std::future::poll_fn(|cx| {
        loop {
            let mut progress = false;

            // 1. 持续驱动 poll_next_inbound
            loop {
                match connection.poll_next_inbound(cx) {
                    Poll::Ready(Some(Ok(stream))) => {
                        if inbound_tx.try_send(stream).is_err() {
                            warn!("tcp yamux driver: inbound channel full or closed");
                        }
                        progress = true;
                        continue;
                    }
                    Poll::Ready(Some(Err(e))) => {
                        return Poll::Ready(format!("yamux error: {}", e));
                    }
                    Poll::Ready(None) => {
                        return Poll::Ready("connection closed by peer".to_string());
                    }
                    Poll::Pending => break,
                }
            }

            // 2. 接收新的出站流请求
            while let Poll::Ready(Some(req)) = outbound_rx.poll_recv(cx) {
                pending_outbound.push(req);
                progress = true;
            }

            // 3. 处理待完成的出站流请求
            while !pending_outbound.is_empty() {
                match connection.poll_new_outbound(cx) {
                    Poll::Ready(Ok(stream)) => {
                        let req = pending_outbound.swap_remove(0);
                        let _ = req.response_tx.send(Ok(stream));
                        progress = true;
                    }
                    Poll::Ready(Err(e)) => {
                        let req = pending_outbound.swap_remove(0);
                        let _ = req.response_tx.send(Err(anyhow!("outbound error: {}", e)));
                        progress = true;
                    }
                    Poll::Pending => {
                        break;
                    }
                }
            }

            if !progress {
                break;
            }
        }

        if outbound_rx.is_closed() && pending_outbound.is_empty() && inbound_tx.is_closed() {
            return Poll::Ready("all handles dropped".to_string());
        }

        Poll::Pending
    })
    .await;

    debug!("tcp yamux driver ended: {}", reason);
    let _ = close_reason_tx.send(Some(reason));
}

#[async_trait]
impl TunnelConnection for TcpTunnelConnection {
    async fn open_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        let (response_tx, response_rx) = oneshot::channel();

        self.outbound_tx
            .send(OutboundRequest { response_tx })
            .await
            .map_err(|_| anyhow!("connection driver closed"))?;

        let stream = response_rx
            .await
            .map_err(|_| anyhow!("connection driver closed"))??;

        let (reader, writer) = stream.split();
        Ok((
            Box::new(TcpTunnelSendStream::new(writer)),
            Box::new(TcpTunnelRecvStream::new(reader)),
        ))
    }

    async fn accept_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)> {
        let stream = {
            let mut rx = self.inbound_rx.lock().await;
            rx.recv().await.ok_or_else(|| anyhow!("connection closed"))?
        };

        let (reader, writer) = stream.split();
        Ok((
            Box::new(TcpTunnelSendStream::new(writer)),
            Box::new(TcpTunnelRecvStream::new(reader)),
        ))
    }

    async fn open_bi_stream(&self) -> Result<Box<dyn TunnelStream>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.outbound_tx
            .send(OutboundRequest { response_tx })
            .await
            .map_err(|_| anyhow!("connection driver closed"))?;

        let stream = response_rx
            .await
            .map_err(|_| anyhow!("connection driver closed"))??;

        // yamux::Stream 实现 futures::AsyncRead + AsyncWrite；compat() 转为 tokio 版本
        Ok(Box::new(stream.compat()))
    }

    async fn accept_bi_stream(&self) -> Result<Box<dyn TunnelStream>> {
        let stream = {
            let mut rx = self.inbound_rx.lock().await;
            rx.recv().await.ok_or_else(|| anyhow!("connection closed"))?
        };

        Ok(Box::new(stream.compat()))
    }

    async fn open_uni(&self) -> Result<Box<dyn TunnelSendStream>> {
        let (response_tx, response_rx) = oneshot::channel();

        self.outbound_tx
            .send(OutboundRequest { response_tx })
            .await
            .map_err(|_| anyhow!("connection driver closed"))?;

        let stream = response_rx
            .await
            .map_err(|_| anyhow!("connection driver closed"))??;

        let (_reader, writer) = stream.split();
        Ok(Box::new(TcpTunnelSendStream::new(writer)))
    }

    async fn accept_uni(&self) -> Result<Box<dyn TunnelRecvStream>> {
        let stream = {
            let mut rx = self.inbound_rx.lock().await;
            rx.recv().await.ok_or_else(|| anyhow!("connection closed"))?
        };

        let (reader, _writer) = stream.split();
        Ok(Box::new(TcpTunnelRecvStream::new(reader)))
    }

    fn remote_address(&self) -> SocketAddr {
        self.remote_addr
    }

    fn close_reason(&self) -> Option<String> {
        self.close_reason_rx.borrow().clone()
    }
}

/// TCP 客户端连接器
pub struct TcpTunnelConnector;

impl TcpTunnelConnector {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl TunnelConnector for TcpTunnelConnector {
    async fn connect(&self, addr: SocketAddr) -> Result<Box<dyn TunnelConnection>> {
        let stream = tokio::net::TcpStream::connect(addr).await?;
        apply_tcp_socket_opts(&stream);
        Ok(Box::new(TcpTunnelConnection::new(stream, addr, true)))
    }
}

/// TCP 服务端监听器
pub struct TcpTunnelListener {
    listener: tokio::net::TcpListener,
}

impl TcpTunnelListener {
    pub async fn new(bind_addr: SocketAddr) -> Result<Self> {
        let listener = tokio::net::TcpListener::bind(bind_addr).await?;
        Ok(Self { listener })
    }
}

#[async_trait]
impl TunnelListener for TcpTunnelListener {
    async fn accept(&self) -> Result<Box<dyn TunnelConnection>> {
        let (stream, addr) = self.listener.accept().await?;
        apply_tcp_socket_opts(&stream);
        Ok(Box::new(TcpTunnelConnection::new(stream, addr, false)))
    }
}
