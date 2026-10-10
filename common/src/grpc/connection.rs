//! Supervise a single Controller stream, including application-level liveness.

use std::future::Future;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub const AUTH_TIMEOUT: Duration = Duration::from_secs(30);
pub const SEND_TIMEOUT: Duration = Duration::from_secs(10);
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(45);

/// Both tasks belong to this stream. Ending either drops the other, closing
/// its channels and notifying the reconnect loop even on a silent network loss.
pub fn supervise<M, F>(
    receive: impl Future<Output = ()> + Send + 'static,
    sender: mpsc::Sender<M>,
    responses: watch::Receiver<Instant>,
    make_heartbeat: F,
) -> CancellationToken
where
    M: Send + 'static,
    F: Fn() -> M + Send + Sync + 'static,
{
    let disconnected = CancellationToken::new();
    let guard = disconnected.clone().drop_guard();
    tokio::spawn(async move {
        let _guard = guard;
        tokio::select! {
            _ = receive => {}
            result = heartbeat_loop(sender, responses, make_heartbeat) => {
                if let Err(error) = result {
                    warn!(%error, "Controller 心跳失败，关闭连接并重连");
                }
            }
        }
    });
    disconnected
}

async fn heartbeat_loop<M>(
    sender: mpsc::Sender<M>,
    mut responses: watch::Receiver<Instant>,
    make_heartbeat: impl Fn() -> M,
) -> Result<()> {
    let first_heartbeat = *responses.borrow() + HEARTBEAT_INTERVAL;
    let mut interval = tokio::time::interval_at(first_heartbeat, HEARTBEAT_INTERVAL);
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);

    loop {
        let deadline = *responses.borrow_and_update() + HEARTBEAT_TIMEOUT;
        tokio::select! {
            biased;
            result = responses.changed() => {
                result.map_err(|_| anyhow!("心跳响应通道已关闭"))?;
            }
            _ = tokio::time::sleep_until(deadline) => {
                bail!("{} 秒未收到 Controller 心跳响应", HEARTBEAT_TIMEOUT.as_secs());
            }
            result = async {
                interval.tick().await;
                sender.send(make_heartbeat()).await
            } => {
                result.map_err(|_| anyhow!("心跳发送通道已关闭"))?;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn missing_responses_close_a_stream_even_when_sends_succeed() {
        let (sender, mut messages) = mpsc::channel(16);
        let (_responses, responses) = watch::channel(Instant::now());
        let (updates, mut updates_rx) = mpsc::channel::<()>(1);
        let disconnected = supervise(
            async move {
                let _updates = updates;
                std::future::pending::<()>().await;
            },
            sender,
            responses,
            || (),
        );

        assert!(messages.recv().await.is_some());
        disconnected.cancelled().await;
        assert!(updates_rx.recv().await.is_none());
        while messages.recv().await.is_some() {}
    }

    #[tokio::test(start_paused = true)]
    async fn responses_keep_the_session_alive_and_eof_stops_heartbeats() {
        let (sender, mut messages) = mpsc::channel(16);
        let (responses, responses_rx) = watch::channel(Instant::now());
        let (end, ended) = tokio::sync::oneshot::channel();
        let disconnected = supervise(
            async {
                let _ = ended.await;
            },
            sender,
            responses_rx,
            || (),
        );

        for _ in 0..8 {
            assert!(messages.recv().await.is_some());
            responses.send_replace(Instant::now());
            tokio::task::yield_now().await;
            assert!(!disconnected.is_cancelled());
        }
        end.send(()).unwrap();
        disconnected.cancelled().await;
        assert!(messages.recv().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_outbound_queue_cannot_block_liveness_detection() {
        let (sender, _messages) = mpsc::channel(1);
        sender.send(()).await.unwrap();
        let (_responses, responses) = watch::channel(Instant::now());
        let disconnected = supervise(std::future::pending(), sender, responses, || ());
        tokio::task::yield_now().await;
        let started = Instant::now();
        disconnected.cancelled().await;
        assert_eq!(started.elapsed(), HEARTBEAT_TIMEOUT);
    }
}
