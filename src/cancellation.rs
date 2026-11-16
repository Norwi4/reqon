//! Cooperative cancellation shared by the engine, scripts, GUI and CLI.
pub use tokio_util::sync::CancellationToken;

#[derive(Debug)]
pub(crate) struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Request cancelled")
    }
}
impl std::error::Error for Cancelled {}

pub(crate) fn check(token: &CancellationToken) -> anyhow::Result<()> {
    if token.is_cancelled() {
        Err(Cancelled.into())
    } else {
        Ok(())
    }
}

pub(crate) async fn run<T>(
    token: &CancellationToken,
    future: impl std::future::Future<Output = anyhow::Result<T>>,
) -> anyhow::Result<T> {
    tokio::select! {
        biased;
        _ = token.cancelled() => Err(Cancelled.into()),
        result = future => result,
    }
}

pub(crate) fn reply() -> crate::model::Reply {
    crate::model::Reply {
        cancelled: true,
        reason: "Cancelled".into(),
        error: Cancelled.to_string(),
        ..Default::default()
    }
}

/// Dropping a desktop task also interrupts any blocking script it started.
#[cfg(feature = "desktop")]
pub(crate) struct Task {
    handle: tokio::task::AbortHandle,
    token: CancellationToken,
}
#[cfg(feature = "desktop")]
impl Task {
    pub fn new(handle: tokio::task::AbortHandle, token: CancellationToken) -> Self {
        Self { handle, token }
    }
    pub fn abort(&self) {
        self.token.cancel();
        self.handle.abort();
    }
}
#[cfg(feature = "desktop")]
impl Drop for Task {
    fn drop(&mut self) {
        self.abort();
    }
}
