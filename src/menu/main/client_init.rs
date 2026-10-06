use crate::client::{
    Client, ClientInitStage, ServerInfo,
    addr::ConnectionArgs,
    error::{Error as ClientError, NetworkConnectError, NetworkError},
};
use common_net::msg::ClientType;
use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use std::path::Path;
use alloc::sync::Arc;
use core::{sync::atomic::AtomicBool, sync::atomic::Ordering, time::Duration};
use tokio::runtime;
use tracing::{trace, warn};

#[derive(Debug)]
#[expect(clippy::enum_variant_names)] //TODO: evaluate ClientError ends with Enum name
pub enum Error {
    ClientError {
        error: ClientError,
        mismatched_server_info: Option<ServerInfo>,
    },
    ClientCrashed,
    ServerNotFound,
}

#[expect(clippy::large_enum_variant)]
pub enum Msg {
    IsAuthTrusted(String),
    Done(Result<Client, Error>),
}

pub struct AuthTrust(String, bool);

// Used to asynchronously parse the server address, resolve host names,
// and create the client (which involves establishing a connection to the
// server).
pub struct ClientInit {
    rx: Receiver<Msg>,
    stage_rx: Receiver<ClientInitStage>,
    trust_tx: Sender<AuthTrust>,
    cancel: Arc<AtomicBool>,
    abort_handle: tokio::task::AbortHandle,
}
impl ClientInit {
    pub fn new(
        connection_args: ConnectionArgs,
        username: String,
        password: String,
        runtime: Arc<runtime::Runtime>,
        locale: Option<String>,
        config_dir: &Path,
        client_type: ClientType,
    ) -> Self {
        Self::start(connection_args, username, password, runtime, locale, config_dir, client_type, false)
    }


    pub fn new_portal(
        connection_args: ConnectionArgs,
        username: String,
        password: String,
        runtime: Arc<runtime::Runtime>,
        locale: Option<String>,
        config_dir: &Path,
        client_type: ClientType,
    ) -> Self {
        Self::start(connection_args, username, password, runtime, locale, config_dir, client_type, true)
    }


    fn start(
        connection_args: ConnectionArgs,
        username: String,
        password: String,
        runtime: Arc<runtime::Runtime>,
        locale: Option<String>,
        config_dir: &Path,
        client_type: ClientType,
        portal: bool,
    ) -> Self {
        let (tx, rx) = unbounded();
        let (trust_tx, trust_rx) = unbounded();
        let (init_stage_tx, init_stage_rx) = unbounded();
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel2 = Arc::clone(&cancel);

        let runtime2 = Arc::clone(&runtime);
        let config_dir = config_dir.to_path_buf();

        let password = zeroize::Zeroizing::new(password);
        let _task = runtime.spawn(async move {
            // This TRUEOS build approves the game server's authentication
            // provider directly, without a UI prompt or saved trust-list gate.
            #[cfg(target_os = "trueos")]
            let trust_fn = {
                drop(trust_rx);
                move |auth_server: &str| !portal || auth_server == crate::server_portal::OFFICIAL_AUTH_SERVER
            };
            #[cfg(not(target_os = "trueos"))]
            let trust_fn = |auth_server: &str| {
                if portal { return auth_server == crate::server_portal::OFFICIAL_AUTH_SERVER; }
                let _ = tx.send(Msg::IsAuthTrusted(auth_server.to_string()));
                trust_rx
                    .recv()
                    .map(|AuthTrust(server, trust)| trust && server == *auth_server)
                    .unwrap_or(false)
            };

            let mut last_err = None;

            const FOUR_MINUTES_RETRIES: u64 = 48;
            'tries: for _ in 0..if portal { 1 } else { FOUR_MINUTES_RETRIES } {
                if cancel2.load(Ordering::Relaxed) {
                    break;
                }
                let mut mismatched_server_info = None;
                match Client::new_with_protocol(
                    connection_args.clone(),
                    Arc::clone(&runtime2),
                    &mut mismatched_server_info,
                    &username,
                    &password,
                    locale.clone(),
                    trust_fn,
                    &|stage| {
                        let _ = init_stage_tx.send(stage);
                    },
                    crate::ecs::sys::add_local_systems,
                    config_dir.clone(),
                    client_type,
                    if portal { crate::client::AdmissionProtocol::Upstream } else { crate::client::AdmissionProtocol::Native },
                )
                .await
                {
                    Ok(client) => {
                        let _ = tx.send(Msg::Done(Ok(client)));
                        tokio::task::block_in_place(move || drop(runtime2));
                        return;
                    },
                    Err(ClientError::NetworkErr(NetworkError::ConnectFailed(
                        NetworkConnectError::Io(e),
                    ))) => {
                        // A closed/refused endpoint will not complete this
                        // attempt. Return its useful error to the login dialog
                        // instead of silently retrying for four minutes.
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotConnected
                        ) {
                            last_err = Some(Error::ClientError {
                                error: ClientError::NetworkErr(NetworkError::ConnectFailed(
                                    NetworkConnectError::Io(e),
                                )),
                                mismatched_server_info,
                            });
                            break 'tries;
                        }
                        warn!(?e, "Failed to connect to the server. Retrying...");
                    },
                    Err(e) => {
                        trace!(?e, "Aborting server connection attempt");
                        last_err = Some(Error::ClientError {
                            error: e,
                            mismatched_server_info,
                        });
                        break 'tries;
                    },
                }
                tokio::time::sleep(Duration::from_secs(5)).await;
            }

            // Parsing/host name resolution successful but no connection succeeded
            // If last_err is None this typically means there was no server up at the input
            // address and all the attempts timed out.
            let _ = tx.send(Msg::Done(Err(last_err.unwrap_or(Error::ServerNotFound))));

            // Safe drop runtime
            tokio::task::block_in_place(move || drop(runtime2));
        });

        ClientInit {
            rx,
            stage_rx: init_stage_rx,
            trust_tx,
            cancel,
            abort_handle: _task.abort_handle(),
        }
    }

    /// Poll if the thread is complete.
    /// Returns None if the thread is still running, otherwise returns the
    /// Result of client creation.
    pub fn poll(&self) -> Option<Msg> {
        match self.rx.try_recv() {
            Ok(msg) => Some(msg),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Msg::Done(Err(Error::ClientCrashed))),
        }
    }

    /// Poll for connection stage updates from the client
    pub fn stage_update(&self) -> Option<ClientInitStage> { self.stage_rx.try_recv().ok() }

    /// Report trust status of auth server
    pub fn auth_trust(&self, auth_server: String, trusted: bool) {
        let _ = self.trust_tx.send(AuthTrust(auth_server, trusted));
    }

    pub fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        // Stop the current handshake/login future as well as future retries.
        self.abort_handle.abort();
    }
}

impl Drop for ClientInit {
    fn drop(&mut self) { self.cancel(); }
}
