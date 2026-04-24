//! 请求-响应关联工具
//!
//! 在双向 gRPC 流上，通过 request_id 关联请求和响应。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

/// 管理双向流上的待处理请求
pub struct PendingRequests<T: Send + 'static> {
    pending: Arc<Mutex<HashMap<String, PendingEntry<T>>>>,
}

struct PendingEntry<T> {
    sender: oneshot::Sender<T>,
    created_at: Instant,
}

/// 过期请求的默认超时时间
const STALE_REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

/// 后台清理间隔
const CLEANUP_INTERVAL: Duration = Duration::from_secs(30);

impl<T: Send + 'static> PendingRequests<T> {
    pub fn new() -> Self {
        let pending: Arc<Mutex<HashMap<String, PendingEntry<T>>>> =
            Arc::new(Mutex::new(HashMap::new()));

        // 启动后台清理任务，定期扫描过期请求
        // 使用 Weak 引用：所有克隆都释放时清理任务自动退出
        let weak = Arc::downgrade(&pending);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
            interval.tick().await; // 跳过首次立即触发
            loop {
                interval.tick().await;
                let Some(map) = weak.upgrade() else { break };
                let mut guard = map.lock().await;
                let now = Instant::now();
                guard.retain(|_, entry| now.duration_since(entry.created_at) < STALE_REQUEST_TIMEOUT);
            }
        });

        Self { pending }
    }

    /// 注册一个待处理请求，返回 (request_id, receiver)
    ///
    /// 热路径：不再顺带扫描 HashMap（由后台任务负责）。
    pub async fn register(&self) -> (String, oneshot::Receiver<T>) {
        let request_id = Uuid::new_v4().to_string();
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        pending.insert(request_id.clone(), PendingEntry {
            sender: tx,
            created_at: Instant::now(),
        });
        (request_id, rx)
    }

    /// 用响应完成一个待处理请求
    /// 返回 true 表示成功匹配，false 表示 request_id 不存在
    pub async fn complete(&self, request_id: &str, response: T) -> bool {
        if let Some(entry) = self.pending.lock().await.remove(request_id) {
            entry.sender.send(response).is_ok()
        } else {
            false
        }
    }

    /// 等待响应，带超时
    pub async fn wait(
        rx: oneshot::Receiver<T>,
        timeout: Duration,
    ) -> Result<T, anyhow::Error> {
        tokio::time::timeout(timeout, rx)
            .await
            .map_err(|_| anyhow::anyhow!("请求超时"))?
            .map_err(|_| anyhow::anyhow!("响应通道已关闭"))
    }
}

impl<T: Send + 'static> Clone for PendingRequests<T> {
    fn clone(&self) -> Self {
        Self {
            pending: self.pending.clone(),
        }
    }
}
