//! 节点日志缓冲区
//!
//! 提供内存中的日志缓冲区，用于跨平台日志查询。

use std::collections::VecDeque;
use std::sync::Arc;
use parking_lot::Mutex;
use tracing::{Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use common::protocol::control::LogEntry;

/// 内存日志缓冲区（环形缓冲区，最多保存 N 条日志）
#[derive(Clone)]
pub struct NodeLogBuffer {
    inner: Arc<Mutex<VecDeque<LogEntry>>>,
    max_size: usize,
}

impl NodeLogBuffer {
    /// 创建新的日志缓冲区
    pub fn new(max_size: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(max_size))),
            max_size,
        }
    }

    /// 添加日志条目
    pub fn push(&self, entry: LogEntry) {
        // parking_lot::Mutex 不会中毒，无需 .unwrap()
        let mut buffer = self.inner.lock();
        if buffer.len() >= self.max_size {
            buffer.pop_front();
        }
        buffer.push_back(entry);
    }

    /// 获取最后 N 条日志
    pub fn get_last(&self, count: usize) -> Vec<LogEntry> {
        let buffer = self.inner.lock();
        let start = buffer.len().saturating_sub(count);
        buffer.iter().skip(start).cloned().collect()
    }

    /// 获取所有日志
    pub fn get_all(&self) -> Vec<LogEntry> {
        let buffer = self.inner.lock();
        buffer.iter().cloned().collect()
    }
}

/// Tracing Layer 实现，将日志写入内存缓冲区
pub struct NodeLogLayer {
    buffer: NodeLogBuffer,
}

impl NodeLogLayer {
    pub fn new(buffer: NodeLogBuffer) -> Self {
        Self { buffer }
    }
}

impl<S: Subscriber> Layer<S> for NodeLogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = metadata.level();

        // 只记录 INFO 及以上级别的日志
        if *level > Level::INFO {
            return;
        }

        // 提取日志消息
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        let entry = LogEntry {
            timestamp: chrono::Utc::now().to_rfc3339(),
            level: level_to_string(level),
            message: visitor.message,
        };

        self.buffer.push(entry);
    }
}

/// 日志访问器，用于提取日志消息
#[derive(Default)]
struct LogVisitor {
    message: String,
}

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
            // 移除 Debug 格式的引号
            if self.message.starts_with('"') && self.message.ends_with('"') {
                self.message = self.message[1..self.message.len() - 1].to_string();
            }
        }
    }
}

fn level_to_string(level: &Level) -> String {
    match *level {
        Level::ERROR => "ERROR".to_string(),
        Level::WARN => "WARN".to_string(),
        Level::INFO => "INFO".to_string(),
        Level::DEBUG => "DEBUG".to_string(),
        Level::TRACE => "TRACE".to_string(),
    }
}

/// 全局日志缓冲区实例
static GLOBAL_LOG_BUFFER: std::sync::OnceLock<NodeLogBuffer> = std::sync::OnceLock::new();

/// 初始化全局日志缓冲区
pub fn init_global_log_buffer(max_size: usize) -> NodeLogBuffer {
    let buffer = NodeLogBuffer::new(max_size);
    let _ = GLOBAL_LOG_BUFFER.set(buffer.clone());
    buffer
}

/// 获取全局日志缓冲区
pub fn get_global_log_buffer() -> Option<NodeLogBuffer> {
    GLOBAL_LOG_BUFFER.get().cloned()
}
