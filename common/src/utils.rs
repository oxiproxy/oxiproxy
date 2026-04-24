use anyhow::Result;
use std::net::SocketAddr;
use std::time::Duration;
use socket2::{Socket, Domain, Type, Protocol};

/// 重连退避策略：指数退避 + 抖动
///
/// 参考 rathole 的 `run_control_chan_backoff`（src/constants.rs:18-26）。
/// 避免所有节点/客户端在 Controller 宕机时同步重试造成的 "thundering herd"。
///
/// 行为：
/// - 首次等待 `initial`
/// - 每次失败翻倍，上限 `max`
/// - 每次叠加 ±20% 随机抖动
#[derive(Clone, Debug)]
pub struct ReconnectBackoff {
    current: Duration,
    initial: Duration,
    max: Duration,
}

impl ReconnectBackoff {
    /// 创建新的退避策略
    ///
    /// 推荐参数：initial=1s, max=60s
    pub fn new(initial: Duration, max: Duration) -> Self {
        Self {
            current: initial,
            initial,
            max,
        }
    }

    /// 默认参数（initial=1s, max=60s）
    pub fn default_params() -> Self {
        Self::new(Duration::from_secs(1), Duration::from_secs(60))
    }

    /// 获取下一次等待时长并更新内部状态
    pub fn next_delay(&mut self) -> Duration {
        use rand::Rng;
        let base = self.current;
        // ±20% 抖动
        let jitter_ratio: f64 = rand::rng().random_range(-0.2..=0.2);
        let base_ms = base.as_millis() as f64;
        let delay_ms = (base_ms * (1.0 + jitter_ratio)).max(0.0) as u64;

        // 为下一次做翻倍
        self.current = (self.current * 2).min(self.max);

        Duration::from_millis(delay_ms)
    }

    /// 连接成功后重置
    pub fn reset(&mut self) {
        self.current = self.initial;
    }
}

#[cfg(windows)]
fn apply_windows_udp_fix(socket: &Socket) -> Result<()> {
    use std::os::windows::io::AsRawSocket;
    use windows_sys::Win32::Networking::WinSock::{WSAIoctl, SIO_UDP_CONNRESET, SOCKET_ERROR};
    use std::ptr;

    let handle = socket.as_raw_socket();
    let mut bytes_returned: u32 = 0;
    let mut enable: u32 = 0; // FALSE

    unsafe {
        let ret = WSAIoctl(
            handle as usize,
            SIO_UDP_CONNRESET,
            &mut enable as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
            ptr::null_mut(),
            0,
            &mut bytes_returned,
            ptr::null_mut(),
            None,
        );

        if ret == SOCKET_ERROR {
            return Err(anyhow::anyhow!("WSAIoctl failed"));
        }
    }
    Ok(())
}

pub async fn create_configured_udp_socket(addr: SocketAddr) -> Result<tokio::net::UdpSocket> {
    let domain = if addr.is_ipv4() { Domain::IPV4 } else { Domain::IPV6 };
    let socket = Socket::new(domain, Type::DGRAM, Some(Protocol::UDP))?;

    socket.set_nonblocking(true)?;

    #[cfg(windows)]
    if let Err(e) = apply_windows_udp_fix(&socket) {
        // Log warning but don't fail? Or fail?
        // Since this is critical for stability on Windows, we should probably fail or at least log.
        // But we don't have tracing here easily unless we add it.
        // Let's just return error.
        return Err(e);
    }

    socket.bind(&addr.into())?;

    let std_socket: std::net::UdpSocket = socket.into();
    let tokio_socket = tokio::net::UdpSocket::from_std(std_socket)?;
    Ok(tokio_socket)
}
