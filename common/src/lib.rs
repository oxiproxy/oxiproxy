//! OxiProxy 公共库
//!
//! 此库包含客户端和服务端共享的代码，
//! 包括隧道协议抽象、KCP 配置和协议类型定义。

pub mod tunnel;
pub mod config;
pub mod utils;
pub mod protocol;
pub mod grpc;


pub use tunnel::{
    TunnelProtocol,
    TunnelSendStream,
    TunnelRecvStream,
    TunnelConnection,
    TunnelConnector,
    TunnelListener,
    QuicSendStream,
    QuicRecvStream,
    QuicConnection,
    QuicConnector,
    QuicListener,
    CertVerificationMode,
    KcpSendStream,
    KcpRecvStream,
    KcpConnection,
    KcpConnector,
    KcpListener,
    TcpTunnelConnector,
    TcpTunnelListener,
};

pub use config::KcpConfig;
