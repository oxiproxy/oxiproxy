//! 隧道抽象 trait 定义
//!
//! 此模块定义了隧道层的统一接口，包括发送流、接收流、连接、
//! 连接器（客户端）和监听器（服务端）等抽象。

use anyhow::Result;
use async_trait::async_trait;
use std::net::SocketAddr;
use tokio::io::{AsyncRead, AsyncWrite};

/// 双向隧道流接口（原生 tokio I/O）
///
/// 实现 `tokio::io::AsyncRead + AsyncWrite`，可直接用于 `tokio::io::copy_bidirectional`，
/// 热路径零拷贝转发，避免手写 read/write 循环的中间 buffer 与额外 task 调度开销。
///
/// 通过 blanket impl 自动实现所有满足条件的类型。
pub trait TunnelStream: AsyncRead + AsyncWrite + Send + Unpin {}

impl<T: AsyncRead + AsyncWrite + Send + Unpin + ?Sized> TunnelStream for T {}

/// 统一发送流接口
///
/// 提供向隧道写入数据的能力，支持 QUIC 和 KCP 协议。
#[async_trait]
pub trait TunnelSendStream: Send + Sync {
    /// 写入所有字节到流
    ///
    /// # Arguments
    /// * `buf` - 要写入的字节数据
    async fn write_all(&mut self, buf: &[u8]) -> Result<()>;

    /// 刷新流缓冲区
    ///
    /// 确保所有缓冲的数据都已发送。
    async fn flush(&mut self) -> Result<()>;

    /// 结束流（发送结束信号）
    ///
    /// 通知对端此流不会再发送更多数据。
    async fn finish(&mut self) -> Result<()>;
}

/// 统一接收流接口
///
/// 提供从隧道读取数据的能力，支持 QUIC 和 KCP 协议。
#[async_trait]
pub trait TunnelRecvStream: Send + Sync {
    /// 读取精确数量的字节
    ///
    /// # Arguments
    /// * `buf` - 存储读取数据的缓冲区
    ///
    /// # Errors
    /// 如果流在读取完成前关闭，返回错误。
    async fn read_exact(&mut self, buf: &mut [u8]) -> Result<()>;

    /// 读取一些字节
    ///
    /// # Arguments
    /// * `buf` - 存储读取数据的缓冲区
    ///
    /// # Returns
    /// * `Some(n)` - 成功读取 n 字节
    /// * `None` - 流已结束
    async fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>>;
}

/// 统一连接接口
///
/// 表示一个已建立的隧道连接，支持创建和接受双向/单向流。
#[async_trait]
pub trait TunnelConnection: Send + Sync {
    /// 打开一个双向流
    ///
    /// # Returns
    /// 返回 (发送流, 接收流) 元组
    async fn open_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)>;

    /// 接受一个双向流
    ///
    /// 等待对端打开一个双向流。
    ///
    /// # Returns
    /// 返回 (发送流, 接收流) 元组
    async fn accept_bi(&self) -> Result<(Box<dyn TunnelSendStream>, Box<dyn TunnelRecvStream>)>;

    /// 打开一个双向流（tokio I/O 版本，用于 `copy_bidirectional` 热路径）
    ///
    /// 返回一个同时实现 `AsyncRead + AsyncWrite` 的对象，
    /// 避免 `TunnelSendStream`/`TunnelRecvStream` 拆分带来的双 task 开销。
    async fn open_bi_stream(&self) -> Result<Box<dyn TunnelStream>>;

    /// 接受一个双向流（tokio I/O 版本，用于 `copy_bidirectional` 热路径）
    async fn accept_bi_stream(&self) -> Result<Box<dyn TunnelStream>>;

    /// 打开一个单向流（只发送）
    async fn open_uni(&self) -> Result<Box<dyn TunnelSendStream>>;

    /// 接受一个单向流（只接收）
    async fn accept_uni(&self) -> Result<Box<dyn TunnelRecvStream>>;

    /// 获取远程地址
    fn remote_address(&self) -> SocketAddr;

    /// 检查连接是否已关闭，返回关闭原因
    ///
    /// # Returns
    /// * `Some(reason)` - 连接已关闭，附带关闭原因
    /// * `None` - 连接仍然活跃
    fn close_reason(&self) -> Option<String>;
}

/// 客户端连接器接口
///
/// 用于客户端连接到服务器。
#[async_trait]
pub trait TunnelConnector: Send + Sync {
    /// 连接到服务器
    ///
    /// # Arguments
    /// * `addr` - 服务器地址
    ///
    /// # Returns
    /// 返回建立的连接
    async fn connect(&self, addr: SocketAddr) -> Result<Box<dyn TunnelConnection>>;
}

/// 服务端监听器接口
///
/// 用于服务端接受客户端连接。
#[async_trait]
pub trait TunnelListener: Send + Sync {
    /// 接受一个新连接
    ///
    /// 阻塞等待直到有新的客户端连接。
    async fn accept(&self) -> Result<Box<dyn TunnelConnection>>;
}
