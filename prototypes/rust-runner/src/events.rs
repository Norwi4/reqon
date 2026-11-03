//! A slow/closed GUI or CI stdout pipe must never block execution/cancellation.
use serde_json::Value;
use std::{
    io::Write,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};

pub struct Events {
    sender: Option<mpsc::SyncSender<Value>>,
    done: mpsc::Receiver<()>,
    dropped: Arc<AtomicU64>,
    run_id: String,
}
impl Events {
    pub fn new(run_id: String) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Value>(256);
        let (finished, done) = mpsc::channel();
        let dropped = Arc::new(AtomicU64::new(0));
        std::thread::spawn(move || {
            let mut stdout = std::io::stdout().lock();
            for event in receiver {
                if writeln!(stdout, "{event}")
                    .and_then(|_| stdout.flush())
                    .is_err()
                {
                    break;
                }
            }
            let _ = finished.send(());
        });
        Self {
            sender: Some(sender),
            done,
            dropped,
            run_id,
        }
    }
    pub fn emit(&self, mut event: Value) {
        event["run_id"] = Value::String(self.run_id.clone());
        if self
            .sender
            .as_ref()
            .is_some_and(|s| s.try_send(event).is_err())
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
    pub fn finish(mut self) {
        self.sender.take();
        let _ = self.done.recv_timeout(Duration::from_millis(250));
    }
}
