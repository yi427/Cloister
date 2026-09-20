//! Active, bounded container-to-host connectivity check with no Agent credentials.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use axum::{Router, http::StatusCode, routing::get};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

use crate::{
    cli::signals::SignalGuard,
    host_bridge::BridgeToken,
    profile::Profile,
    runtime::{HOST_BRIDGE_GUEST_NAME, execute_managed, host_connectivity_command},
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(20);

pub(super) async fn probe(profile: &Profile) -> Result<(), String> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|e| format!("could not bind the temporary host listener: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let nonce = BridgeToken::generate().map_err(|e| e.to_string())?;
    let path = format!("/cloister-check/{}", nonce.secret());
    let endpoint = format!("http://{HOST_BRIDGE_GUEST_NAME}:{port}{path}");
    let name = format!(
        "cloister-check-{}",
        nonce
            .secret()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .take(24)
            .collect::<String>()
    );
    let received = Arc::new(AtomicBool::new(false));
    let observed = Arc::clone(&received);
    let router = Router::new().route(
        &path,
        get(move || async move {
            observed.store(true, Ordering::SeqCst);
            StatusCode::NO_CONTENT
        }),
    );
    let _server = ProbeServer(tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    }));
    let cancellation = CancellationToken::new();
    let _signals = SignalGuard::start(cancellation.clone()).map_err(|e| e.to_string())?;
    let command = host_connectivity_command(profile, &name, &endpoint);
    println!("Checking host connectivity with a temporary container (20s timeout)...");
    let execution = execute_managed(&command, &name, &cancellation, true);
    tokio::pin!(execution);
    let status = tokio::select! {
        status = &mut execution => status.map_err(|e| format!("connectivity probe failed or was interrupted: {e}"))?,
        () = tokio::time::sleep(PROBE_TIMEOUT) => {
            cancellation.cancel();
            // Await managed cleanup rather than dropping a running container client.
            let _ = execution.await;
            return Err("connectivity probe exceeded its 20s deadline; container cleanup was requested".into());
        }
    };
    if !status.success() {
        return Err(format!(
            "probe container failed ({status}); DNS, TCP/HTTP forwarding, or the image's /usr/bin/curl may be unavailable"
        ));
    }
    if !received.load(Ordering::SeqCst) {
        return Err("probe exited successfully but the temporary host listener received no matching request".into());
    }
    Ok(())
}

struct ProbeServer(JoinHandle<()>);
impl Drop for ProbeServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}
