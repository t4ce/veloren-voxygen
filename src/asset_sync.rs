//! Manual asset sync. Networking and redb work never run on the UI lane.
//! A CAS is the cancellation boundary: once Writing wins, UI exit is blocked
//! until the native atomic file commit and RAM source replacement finish.
use alloc::sync::Arc;
use common::assets::{
    self,
    sync::{self, Index},
};
use std::{
    future::Future,
    io::{self, Write},
    sync::{
        Mutex,
        atomic::{AtomicU8, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

const RUNNING: u8 = 0;
const CANCELED: u8 = 1;
const WRITING: u8 = 2;
const FINISHED: u8 = 3;

#[derive(Clone)]
pub struct Status {
    pub message: String,
    pub cancellable: bool,
    pub result: Option<Result<String, String>>,
}
struct Shared {
    phase: AtomicU8,
    status: Mutex<Status>,
    wake: tokio::sync::Notify,
}
impl Shared {
    fn check(&self) -> io::Result<()> {
        if self.phase.load(Ordering::Acquire) == CANCELED {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Asset sync canceled",
            ))
        } else {
            Ok(())
        }
    }
    fn progress(&self, message: String) -> io::Result<()> {
        self.check()?;
        self.status.lock().unwrap().message = message;
        Ok(())
    }
    fn begin_write(&self) -> io::Result<()> {
        self.phase
            .compare_exchange(RUNNING, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| io::Error::new(io::ErrorKind::Interrupted, "Asset sync canceled"))?;
        let mut status = self.status.lock().unwrap();
        status.cancellable = false;
        status.message = "Writing asset database to disk…".into();
        Ok(())
    }
}
pub struct Job {
    shared: Arc<Shared>,
}
impl Job {
    pub fn start(host: String, runtime: &Arc<tokio::runtime::Runtime>) -> Self {
        let shared = Arc::new(Shared {
            phase: AtomicU8::new(RUNNING),
            status: Mutex::new(Status {
                message: format!("Contacting asset server on TCP {}…", sync::PORT),
                cancellable: true,
                result: None,
            }),
            wake: tokio::sync::Notify::new(),
        });
        let worker = Arc::clone(&shared);
        runtime.spawn(async move {
            let result = run(&host, &worker).await.map_err(|error| error.to_string());
            let result = if worker.phase.swap(FINISHED, Ordering::AcqRel) == CANCELED {
                Err("Asset sync canceled".into())
            } else {
                result
            };
            let mut status = worker.status.lock().unwrap();
            status.cancellable = false;
            status.result = Some(result);
        });
        Self { shared }
    }
    pub fn status(&self) -> Status {
        self.shared.status.lock().unwrap().clone()
    }
    pub fn writing(&self) -> bool {
        self.shared.phase.load(Ordering::Acquire) == WRITING
    }
    pub fn cancel(&self) -> bool {
        if self
            .shared
            .phase
            .compare_exchange(RUNNING, CANCELED, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.shared.wake.notify_one();
            self.shared.status.lock().unwrap().message = "Canceling asset sync…".into();
            true
        } else {
            false
        }
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel();
    }
}

async fn network<T>(
    shared: &Arc<Shared>,
    future: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    shared.check()?;
    tokio::select! {
        _ = shared.wake.notified() => { shared.check()?; Err(io::Error::new(io::ErrorKind::Interrupted, "Asset sync canceled")) },
        result = tokio::time::timeout(Duration::from_secs(30), future) => result.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "Asset server timed out"))?,
    }
}

async fn run(host: &str, shared: &Arc<Shared>) -> io::Result<String> {
    let addresses = network(shared, crate::client::addr::resolve(host, false))
        .await
        .map_err(|e| io::Error::other(format!("Asset server is not reachable: {e}")))?;
    let mut connected = None;
    for mut address in addresses {
        address.set_port(sync::PORT);
        match network(shared, TcpStream::connect(address)).await {
            Ok(socket) => {
                connected = Some(socket);
                break;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
            Err(_) => {}
        }
    }
    let mut socket = connected.ok_or_else(|| {
        io::Error::other(format!(
            "Asset server is not reachable on TCP {}.",
            sync::PORT
        ))
    })?;
    shared.progress("Checking local asset database…".into())?;
    let worker = Arc::clone(shared);
    let mut local = tokio::task::spawn_blocking(move || {
        assets::asset_sync_snapshot(&mut |key| worker.progress(format!("Checking assets: {key}")))
    })
    .await
    .map_err(io::Error::other)??;
    let mut hello = sync::HELLO.to_vec();
    hello.extend_from_slice(&local.index.root);
    network(shared, socket.write_all(&hello)).await?;
    let status = network(shared, socket.read_u8()).await?;
    if status == 0 {
        return Ok("Assets are already up to date.".into());
    }
    if status != 1 {
        return Err(sync::invalid(
            "Asset server returned an unsupported response",
        ));
    }
    let len = network(shared, socket.read_u32_le()).await? as usize;
    if len > sync::MAX_INDEX {
        return Err(sync::invalid("Asset index exceeds limit"));
    }
    let mut bytes = vec![0; len];
    network(shared, socket.read_exact(&mut bytes)).await?;
    let target = Index::decode(&bytes)?;
    let worker = Arc::clone(shared);
    let (returned, target, indices) = tokio::task::spawn_blocking(move || -> io::Result<_> {
        let indices = local.prune_and_diff(&target, &mut |key| {
            worker.progress(format!("Removing obsolete asset: {key}"))
        })?;
        Ok((local, target, indices))
    })
    .await
    .map_err(io::Error::other)??;
    local = returned;
    let mut request = sync::GET.to_vec();
    request.extend_from_slice(&target.root);
    request.extend_from_slice(&(indices.len() as u32).to_le_bytes());
    for index in &indices {
        request.extend_from_slice(&index.to_le_bytes());
    }
    network(shared, socket.write_all(&request)).await?;
    if network(shared, socket.read_u8()).await? != 2 {
        return Err(sync::invalid("Asset server refused the bundle"));
    }
    let names = indices
        .iter()
        .take(3)
        .map(|i| target.files[*i as usize].key.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut compressed = Vec::new();
    let mut chunk = vec![0; 64 * 1024];
    loop {
        let read = network(shared, socket.read(&mut chunk)).await?;
        if read == 0 {
            break;
        }
        if compressed.len() + read > sync::MAX_IMAGE {
            return Err(sync::invalid("Compressed update exceeds limit"));
        }
        compressed.extend_from_slice(&chunk[..read]);
        shared.progress(format!(
            "Downloading {} assets · {} MiB\n{}",
            indices.len(),
            compressed.len() / (1024 * 1024),
            names
        ))?;
    }
    drop(socket);
    let worker = Arc::clone(shared);
    let (image, packed) = tokio::task::spawn_blocking(move || -> io::Result<_> {
        local.apply_bundle(&target, &indices, &compressed, &mut |key| {
            worker.progress(format!("Verifying and extracting: {key}"))
        })?;
        drop(compressed);
        worker.progress("Preparing updated database…".into())?;
        let image = local.into_image()?;
        worker.check()?;
        let mut encoder = sync::lz4_encoder(Vec::new());
        for chunk in image.chunks(64 * 1024) {
            worker.check()?;
            encoder.write_all(chunk)?;
        }
        let packed = encoder.finish().map_err(io::Error::other)?;
        worker.check()?;
        Ok((image, packed))
    })
    .await
    .map_err(io::Error::other)??;
    shared.begin_write()?;
    let path = std::env::var("VOXYGEN_ASSET_DATABASE")
        .unwrap_or_else(|_| "/apps/voxy/voxygen-assets.redb.lz4".into());
    persist(&path, packed).await?;
    tokio::task::spawn_blocking(move || assets::install_asset_sync_image(image))
        .await
        .map_err(io::Error::other)??;
    Ok("Assets synchronized. Restart Voxy to refresh assets already loaded.".into())
}

#[cfg(target_os = "trueos")]
async fn persist(path: &str, bytes: Vec<u8>) -> io::Result<()> {
    trueos::async_fs::write_file_typed(
        path.as_bytes(),
        &bytes,
        trueos::async_fs::ContentTypeId::LZ4,
    )
    .await
    .map_err(|e| io::Error::other(format!("Asset database write failed ({e})")))
}

#[cfg(all(test, not(target_os = "trueos")))]
mod tests {
    use super::*;
    use std::sync::Barrier;

    fn shared() -> Arc<Shared> {
        Arc::new(Shared {
            phase: AtomicU8::new(RUNNING),
            status: Mutex::new(Status {
                message: String::new(),
                cancellable: true,
                result: None,
            }),
            wake: tokio::sync::Notify::new(),
        })
    }

    #[test]
    fn cancel_and_write_race_have_exactly_one_winner() {
        for _ in 0..64 {
            let state = shared();
            let barrier = Arc::new(Barrier::new(2));
            let writer = Arc::clone(&state);
            let start = Arc::clone(&barrier);
            let worker = std::thread::spawn(move || {
                start.wait();
                writer.begin_write().is_ok()
            });
            let job = Job { shared: state };
            barrier.wait();
            let canceled = job.cancel();
            let writing = worker.join().unwrap();
            assert_ne!(canceled, writing);
            if writing {
                assert!(!job.cancel());
                assert!(!job.status().cancellable);
            } else {
                assert!(job.shared.begin_write().is_err());
            }
        }
    }

    #[test]
    fn cancellation_interrupts_a_waiting_network_read() {
        let state = shared();
        let worker = Arc::clone(&state);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let pending = tokio::spawn(async move {
                network(&worker, std::future::pending::<io::Result<()>>()).await
            });
            tokio::task::yield_now().await;
            let job = Job { shared: state };
            assert!(job.cancel());
            assert_eq!(
                pending.await.unwrap().unwrap_err().kind(),
                io::ErrorKind::Interrupted
            );
        });
    }

    fn fixture(value: &[u8], extra: &str) -> sync::AssetDb {
        let store = picasso::Picasso::new().unwrap();
        store
            .put_embedded_asset("common.canary/canary", b"VELOREN_CANARY_MAGIC")
            .unwrap();
        store.put_embedded_asset("value/ron", value).unwrap();
        store
            .put_embedded_asset(&format!("{extra}/bin"), b"extra")
            .unwrap();
        let catalog = format!(
            "(version:1,directories:{{\"\":[Directory(\"common\"),File(\"value\",\"ron\"),File({extra:?},\"bin\")],\"common\":[File(\"common.canary\",\"canary\")]}},files:[\"common.canary/canary\",\"value/ron\",\"{extra}/bin\"],bytes:{})",
            b"VELOREN_CANARY_MAGIC".len() + value.len() + 5
        );
        store
            .put_embedded_asset(sync::CATALOG, catalog.as_bytes())
            .unwrap();
        sync::AssetDb::from_store(store, &mut |_| Ok(())).unwrap()
    }

    #[test]
    fn real_worker_syncs_and_commits_then_takes_the_matching_hash_fast_path() {
        let root = std::env::temp_dir().join(format!("voxy-sync-proof-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("assets.redb.lz4");
        let old = fixture(b"[1]", "obsolete").into_image().unwrap();
        let mut encoder = sync::lz4_encoder(Vec::new());
        encoder.write_all(&old).unwrap();
        std::fs::write(&path, encoder.finish().unwrap()).unwrap();
        let target = fixture(b"[2]", "added");
        let wanted = target.index.root;
        let listener = std::net::TcpListener::bind(("127.0.0.1", sync::PORT)).unwrap();
        let server = std::thread::spawn(move || {
            let index = target.index.encode().unwrap();
            for _ in 0..2 {
                let (socket, _) = listener.accept().unwrap();
                sync::server::serve(socket, &target, &index).unwrap();
            }
        });
        // A subprocess supplies the database path before the static asset cache
        // initializes; no global environment mutation races other tests.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "production::tests::sync_child", "--nocapture"])
            .env("VOXYGEN_ASSET_DATABASE", &path)
            .env("VOXY_SYNC_PROOF_CHILD", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        server.join().unwrap();
        let committed =
            sync::AssetDb::open(sync::decode_image(std::fs::read(&path).unwrap()).unwrap())
                .unwrap();
        assert_eq!(committed.index.root, wanted);
        assert!(
            committed
                .store
                .embedded_asset("obsolete/bin")
                .unwrap()
                .is_none()
        );
        assert!(
            committed
                .store
                .embedded_asset("added/bin")
                .unwrap()
                .is_some()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn sync_child() {
        if std::env::var_os("VOXY_SYNC_PROOF_CHILD").is_none() {
            return;
        }
        use assets::AssetExt;
        let cached = assets::Ron::<Vec<u32>>::load("value").unwrap();
        assert_eq!(cached.read().0, [1]);
        let runtime = Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap(),
        );
        for expected in ["Assets synchronized", "already up to date"] {
            let job = Job::start("127.0.0.1:9".into(), &runtime);
            let started = std::time::Instant::now();
            loop {
                if let Some(result) = job.status().result {
                    assert!(result.unwrap().contains(expected));
                    break;
                }
                assert!(started.elapsed() < Duration::from_secs(30));
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        assert_eq!(assets::Ron::<Vec<u32>>::load_owned("value").unwrap().0, [2]);
        assert_eq!(cached.read().0, [1]); // Existing handles remain valid.
    }
}
#[cfg(not(target_os = "trueos"))]
async fn persist(path: &str, bytes: Vec<u8>) -> io::Result<()> {
    let path = std::path::PathBuf::from(path);
    tokio::task::spawn_blocking(move || -> io::Result<()> {
        let temporary = path.with_extension(format!("lz4.sync-tmp-{}", std::process::id()));
        let result = (|| {
            let mut file = std::fs::File::create(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(&temporary, &path)?;
            if let Some(parent) = path.parent() {
                std::fs::File::open(parent)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        result
    })
    .await
    .map_err(io::Error::other)?
}
