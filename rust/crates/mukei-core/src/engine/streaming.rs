//! `mukei_core::engine::streaming` — TRD §3.1 (BUGFIX v0.7.4).
//!
//! Tokens emitted by the LLM arrive at extremely high rates. Sending
//! each one through a CXX-Qt signal would be a compile error (the
//! `&mut self` signal handler is not `'static`) and a runtime DoS
//! against the UI thread.
//!
//! Solution: accumulate tokens in a local buffer for ~50 ms (wall
//! clock) and emit a SINGLE chunk per flush via the mpsc channel.
//! The sink on the QML side receives batched strings of up to a few
//! hundred characters per drain.

use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[derive(Copy, Clone, Debug)]
pub struct TokenStreamConfig {
    pub flush_window: Duration,
    pub max_chunk_bytes: usize,
}

impl Default for TokenStreamConfig {
    fn default() -> Self {
        Self {
            flush_window: Duration::from_millis(50),
            max_chunk_bytes: 1_024,
        }
    }
}

/// Pulls from an upstream `mpsc::Receiver<String>` and republishes
/// concatenated chunks at the configured window. Bounded `max_chunk_bytes`
/// guards against pathological 50 kB single-token chunks.
pub struct Drainer {
    cfg: TokenStreamConfig,
}

impl Drainer {
    pub fn new(cfg: TokenStreamConfig) -> Self {
        Self { cfg }
    }

    pub async fn run(
        self,
        mut src: mpsc::Receiver<String>,
        dst: mpsc::Sender<String>,
        cancel: CancellationToken,
    ) {
        let mut buf = String::new();
        let mut tick = tokio::time::interval(self.cfg.flush_window);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => { self.flush(&mut buf, &dst).await; break; }
                _ = tick.tick()         => { self.flush(&mut buf, &dst).await; }
                maybe = src.recv()      => match maybe {
                    Some(piece) => {
                        buf.push_str(&piece);
                        if buf.len() >= self.cfg.max_chunk_bytes {
                            self.flush(&mut buf, &dst).await;
                        }
                    }
                    None        => { self.flush(&mut buf, &dst).await; break; }
                }
            }
        }
    }

    async fn flush(&self, buf: &mut String, dst: &mpsc::Sender<String>) {
        if buf.is_empty() {
            return;
        }
        let _ = dst.send(std::mem::take(buf)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn drains_batches_of_two() {
        let (src_tx, src_rx) = mpsc::channel::<String>(16);
        let (dst_tx, mut dst_rx) = mpsc::channel::<String>(16);
        let cancel = CancellationToken::new();

        let drainer = Drainer::new(TokenStreamConfig {
            flush_window: Duration::from_millis(20),
            max_chunk_bytes: 64,
        });
        let h = tokio::spawn(drainer.run(src_rx, dst_tx, cancel.clone()));

        src_tx.send("hello ".into()).await.unwrap();
        src_tx.send("world".into()).await.unwrap();
        drop(src_tx);

        // Give the drainer one tick to flush.
        sleep(Duration::from_millis(40)).await;

        // At least one chunk should have arrived.
        let mut got = String::new();
        while let Ok(s) = dst_rx.try_recv() {
            got.push_str(&s);
        }
        assert!(got.starts_with("hello "));

        // Let the drainer finish.
        cancel.cancel();
        let _ = h.await;
    }
}
