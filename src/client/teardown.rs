//! Keep graceful connection teardown off the TRUEOS input/Hull thread.
use network::{Network, Participant};
use tokio::{runtime::Runtime, task::JoinHandle};

pub(super) fn retire_connection(
    runtime: &Runtime,
    participant: Option<Participant>,
    network: Option<Network>,
) -> JoinHandle<()> {
    runtime.spawn(async move {
        if let Some(participant) = participant {
            if let Err(error) = participant.disconnect().await {
                tracing::warn!(?error, "Client connection teardown failed to flush all data");
            }
        }
        // Network::drop detects the async runtime context and defers its
        // scheduler acknowledgement rather than synchronously blocking input.
        drop(network);
    })
}
