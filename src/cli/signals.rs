//! Scoped signal handlers for managed container lifecycles.

use std::io;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

pub(super) struct SignalGuard(JoinHandle<()>);
impl SignalGuard {
    pub(super) fn start(cancellation: CancellationToken) -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};
        let mut interrupt = signal(SignalKind::interrupt())?;
        let mut terminate = signal(SignalKind::terminate())?;
        let mut hangup = signal(SignalKind::hangup())?;
        Ok(Self(tokio::spawn(async move {
            tokio::select! { _ = interrupt.recv() => (), _ = terminate.recv() => (), _ = hangup.recv() => () }
            cancellation.cancel();
        })))
    }
}
impl Drop for SignalGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
