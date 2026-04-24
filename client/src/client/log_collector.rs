use std::collections::VecDeque;
use std::sync::Arc;
use parking_lot::Mutex;
use serde::{Serialize, Deserialize};
use tracing::{Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

/// 日志条目
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub level: String,
    pub message: String,
}

/// 日志收集器 - 保存最近的日志到内存
#[derive(Clone)]
pub struct LogCollector {
    logs: Arc<Mutex<VecDeque<LogEntry>>>,
    max_entries: usize,
}

impl LogCollector {
    pub fn new(max_entries: usize) -> Self {
        Self {
            logs: Arc::new(Mutex::new(VecDeque::with_capacity(max_entries))),
            max_entries,
        }
    }

    pub fn add_log(&self, level: String, message: String) {
        // parking_lot::Mutex 不会中毒，无需 .unwrap()
        let mut logs = self.logs.lock();

        // 如果达到最大容量，移除最旧的日志
        if logs.len() >= self.max_entries {
            logs.pop_front();
        }

        logs.push_back(LogEntry {
            timestamp: chrono::Utc::now(),
            level,
            message,
        });
    }

    /// 获取最近的N条日志
    pub fn get_recent_logs(&self, count: usize) -> Vec<LogEntry> {
        let logs = self.logs.lock();
        let start = logs.len().saturating_sub(count);
        logs.iter().skip(start).cloned().collect()
    }

    /// 获取所有日志
    pub fn get_all_logs(&self) -> Vec<LogEntry> {
        let logs = self.logs.lock();
        logs.iter().cloned().collect()
    }
}

/// 自定义 tracing Layer，用于捕获日志
pub struct LogCollectorLayer {
    collector: LogCollector,
}

impl LogCollectorLayer {
    pub fn new(collector: LogCollector) -> Self {
        Self { collector }
    }
}

impl<S> Layer<S> for LogCollectorLayer
where
    S: Subscriber,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: Context<'_, S>,
    ) {
        let metadata = event.metadata();
        let level = match *metadata.level() {
            Level::ERROR => "ERROR",
            Level::WARN => "WARN",
            Level::INFO => "INFO",
            Level::DEBUG => "DEBUG",
            Level::TRACE => "TRACE",
        };

        // 提取日志消息
        let mut visitor = LogVisitor::default();
        event.record(&mut visitor);

        if let Some(message) = visitor.message {
            self.collector.add_log(level.to_string(), message);
        }
    }
}

/// 访问者，用于提取事件中的消息
#[derive(Default)]
struct LogVisitor {
    message: Option<String>,
}

impl tracing::field::Visit for LogVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = Some(format!("{:?}", value));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = Some(value.to_string());
        }
    }
}
