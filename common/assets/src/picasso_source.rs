//! One-time filesystem ingestion, followed by a RAM-only asset source.
//! Raw bytes belong to Picasso; assets_manager still caches decoded objects.

use assets_manager::source::{DirEntry, FileContent, Source};
use picasso::Picasso;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt, io,
    time::Instant,
};

pub(super) struct PicassoSource {
    store: Picasso,
    directories: BTreeMap<String, Vec<Entry>>,
    files: BTreeSet<String>,
    bytes: u64,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(super) enum Entry {
    File(String, String),
    Directory(String),
}

impl Entry {
    pub(super) fn borrowed(&self) -> DirEntry<'_> {
        match self {
            Self::File(id, ext) => DirEntry::File(id, ext),
            Self::Directory(id) => DirEntry::Directory(id),
        }
    }
}

const CATALOG_KEY: &str = "__veloren_asset_catalog/v1";
const MAX_DATABASE_IMAGE_BYTES: usize = 1024 * 1024 * 1024;

fn asset_progress(message: fmt::Arguments<'_>) {
    #[cfg(target_os = "trueos")]
    let _ = trueos::logl::log_record(trueos::logl::level::IMPORTANT, "apps::voxygen", message);
    #[cfg(not(target_os = "trueos"))]
    eprintln!("{message}");
}

#[derive(Serialize, Deserialize)]
struct Catalog {
    version: u32,
    directories: BTreeMap<String, Vec<Entry>>,
    files: BTreeSet<String>,
    bytes: u64,
}

fn asset_key(id: &str, ext: &str) -> String {
    // Asset IDs and extensions cannot contain a path separator.
    format!("{id}/{ext}")
}

fn is_bundle(id: &str, ext: &str) -> bool {
    ext == "redb"
        || ext == "redb.lz4"
        || ext == "redb.lz4.sha256"
        || (ext == "lz4" && id.ends_with(".redb"))
        || (ext == "sha256" && id.ends_with(".redb.lz4"))
        || ext == "tar.lz4"
        || ext == "tar.lz4.sha256"
        || (ext == "lz4" && id.ends_with(".tar"))
        || (ext == "sha256" && id.ends_with(".tar.lz4"))
}

impl PicassoSource {
    pub(super) fn new() -> io::Result<Self> {
        let start = Instant::now();
        asset_progress(format_args!("Voxygen assets: initializing Picasso assets"));
        // Preserve canary validation and override precedence at ingestion.
        // Neither filesystem source nor a path is retained by PicassoSource.
        #[cfg(not(target_os = "trueos"))]
        let source = if let Some(path) = std::env::var_os("VOXYGEN_ASSET_DATABASE") {
            Self::from_database_image(std::fs::read(path)?)?
        } else {
            Self::import(&super::fs::FileSystem::new()?)?
        };
        #[cfg(target_os = "trueos")]
        let source = Self::from_runtime_archive()?;
        asset_progress(format_args!(
            "Voxygen assets: serving assets from RAM files={} bytes={} seconds={:.2}",
            source.files.len(),
            source.bytes,
            start.elapsed().as_secs_f64()
        ));
        Ok(source)
    }

    #[cfg(target_os = "trueos")]
    fn from_runtime_archive() -> io::Result<Self> {
        let path = std::env::var("VOXYGEN_ASSET_DATABASE")
            .unwrap_or_else(|_| "/apps/voxy/voxygen-assets.redb.lz4".into());
        asset_progress(format_args!(
            "Voxygen assets: phase=decode-request path={path}"
        ));
        let started = trueos::clock::Instant::now();
        let mut last_update = started;
        let mut copying = false;
        let bytes = trueos::async_fs::block_on(trueos::archive::decode_lz4_to_memory_with_progress(
            path.as_bytes(),
            MAX_DATABASE_IMAGE_BYTES,
            |progress| {
                use trueos::archive::MemoryProgress;
                let is_copy = matches!(progress, MemoryProgress::Copying { .. });
                let finished = match progress {
                    MemoryProgress::Decoding { percent } => percent == 100,
                    MemoryProgress::Copying { copied, total } => copied == total,
                };
                if is_copy != copying || finished || last_update.elapsed().as_millis() >= 5000 {
                    match progress {
                        MemoryProgress::Decoding { percent } => asset_progress(format_args!(
                            "Voxygen assets: phase=decode-progress percent={percent} elapsed_seconds={}",
                            started.elapsed().as_millis() / 1000
                        )),
                        MemoryProgress::Copying { copied, total } => asset_progress(format_args!(
                            "Voxygen assets: phase=ram-copy copied_mib={} total_mib={} elapsed_seconds={}",
                            copied / (1024 * 1024), total.div_ceil(1024 * 1024),
                            started.elapsed().as_millis() / 1000
                        )),
                    }
                    copying = is_copy;
                    last_update = trueos::clock::Instant::now();
                }
            },
        ))
        .map_err(|code| {
            io::Error::other(format!(
                "LZ4 decode of prepared asset database {path} failed (code {code})"
            ))
        })?;
        let image_bytes = bytes.len();
        asset_progress(format_args!(
            "Voxygen assets: phase=db-open image_bytes={image_bytes} mode=prebuilt"
        ));
        let result = Self::from_database_image(bytes)?;
        asset_progress(format_args!(
            "Voxygen assets: phase=db-ready files={} bytes={} image_bytes={image_bytes} mode=prebuilt",
            result.files.len(),
            result.bytes
        ));
        Ok(result)
    }

    fn from_database_image(bytes: Vec<u8>) -> io::Result<Self> {
        if bytes.len() > MAX_DATABASE_IMAGE_BYTES {
            return Err(io::Error::other(
                "prepared asset database exceeds size limit",
            ));
        }
        let store = Picasso::from_runtime_database_image(bytes).map_err(storage_error)?;
        let raw = store
            .embedded_asset(CATALOG_KEY)
            .map_err(storage_error)?
            .ok_or_else(|| io::Error::other("prepared database has no Veloren asset catalog"))?;
        if raw.len() > 8 * 1024 * 1024 {
            return Err(io::Error::other(
                "prepared asset catalog exceeds size limit",
            ));
        }
        let catalog: Catalog =
            ron::de::from_bytes(&raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if catalog.version != 1
            || catalog.files.len() > 16_384
            || catalog.bytes > 512 * 1024 * 1024
            || !catalog.directories.contains_key("")
        {
            return Err(io::Error::other("invalid prepared asset catalog"));
        }
        let canary = store
            .embedded_asset("common.canary/canary")
            .map_err(storage_error)?
            .ok_or_else(|| io::Error::other("prepared database has no common.canary.canary"))?;
        if !canary.starts_with(b"VELOREN_CANARY_MAGIC") {
            return Err(io::Error::other(
                "prepared database has an invalid Veloren canary",
            ));
        }
        Ok(Self {
            store,
            directories: catalog.directories,
            files: catalog.files,
            bytes: catalog.bytes,
        })
    }

    #[cfg(not(target_os = "trueos"))]
    fn into_database_image(self) -> io::Result<Vec<u8>> {
        let Self {
            store,
            directories,
            files,
            bytes,
        } = self;
        if files.contains(CATALOG_KEY) {
            return Err(io::Error::other(
                "asset conflicts with the prepared database catalog key",
            ));
        }
        let catalog = Catalog {
            version: 1,
            directories,
            files,
            bytes,
        };
        let raw = ron::ser::to_string(&catalog).map_err(io::Error::other)?;
        store
            .put_embedded_asset(CATALOG_KEY, raw.as_bytes())
            .map_err(storage_error)?;
        let image = store.into_runtime_database_image().map_err(storage_error)?;
        if image.len() > MAX_DATABASE_IMAGE_BYTES {
            return Err(io::Error::other(
                "prepared database exceeds runtime size limit",
            ));
        }
        Ok(image)
    }

    pub(super) fn import(source: &impl Source) -> io::Result<Self> {
        let mut result = Self {
            store: Picasso::new().map_err(storage_error)?,
            directories: BTreeMap::new(),
            files: BTreeSet::new(),
            bytes: 0,
        };
        #[cfg(target_os = "trueos")]
        let mut last_update = trueos::clock::Instant::now();
        #[cfg(target_os = "trueos")]
        asset_progress(format_args!("Voxygen assets: phase=db-import"));
        let mut pending = vec![String::new()];
        while let Some(directory) = pending.pop() {
            let mut entries = Vec::new();
            source.read_dir(&directory, &mut |entry| match entry {
                DirEntry::File(id, ext) if !is_bundle(id, ext) => {
                    entries.push(Entry::File(id.to_owned(), ext.to_owned()));
                }
                DirEntry::Directory(id) => entries.push(Entry::Directory(id.to_owned())),
                _ => {}
            })?;
            for entry in &entries {
                match entry {
                    Entry::Directory(id) => pending.push(id.clone()),
                    Entry::File(id, ext) => {
                        let content = source.read(id, ext).map_err(|error| {
                            io::Error::new(error.kind(), format!("importing {id}.{ext}: {error}"))
                        })?;
                        let key = asset_key(id, ext);
                        result
                            .store
                            .put_embedded_asset(&key, content.as_ref())
                            .map_err(storage_error)?;
                        result.files.insert(key);
                        result.bytes += content.as_ref().len() as u64;
                        #[cfg(target_os = "trueos")]
                        if result.files.len() % 1024 == 0
                            || last_update.elapsed().as_millis() >= 5000
                        {
                            asset_progress(format_args!(
                                "Voxygen assets: phase=db-progress files={} bytes={}",
                                result.files.len(),
                                result.bytes
                            ));
                            last_update = trueos::clock::Instant::now();
                            trueos::vsys::poll_once();
                        }
                    }
                }
            }
            result.directories.insert(directory, entries);
        }
        Ok(result)
    }
}

#[cfg(not(target_os = "trueos"))]
pub(super) fn prepare_database(path: &std::path::Path) -> io::Result<()> {
    let start = Instant::now();
    let source = PicassoSource::import(&super::fs::FileSystem::new()?)?;
    let files = source.files.len();
    let bytes = source.bytes;
    let image = source.into_database_image()?;
    let image_bytes = image.len();
    std::fs::write(path, image)?;
    eprintln!(
        "Voxygen assets: phase=db-prepared files={files} bytes={bytes} image_bytes={image_bytes} seconds={:.2} path={}",
        start.elapsed().as_secs_f64(),
        path.display()
    );
    Ok(())
}

fn storage_error(error: picasso::PicassoError) -> io::Error {
    io::Error::other(error.to_string())
}

impl fmt::Debug for PicassoSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PicassoSource")
            .field("files", &self.files.len())
            .field("bytes", &self.bytes)
            .finish_non_exhaustive()
    }
}

impl Source for PicassoSource {
    fn read(&self, id: &str, ext: &str) -> io::Result<FileContent<'_>> {
        self.store
            .embedded_asset(&asset_key(id, ext))
            .map_err(storage_error)?
            .map(FileContent::Buffer)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, format!("asset {id}.{ext}")))
    }

    fn read_dir(&self, id: &str, callback: &mut dyn FnMut(DirEntry)) -> io::Result<()> {
        let entries = self.directories.get(id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("asset directory {id}"))
        })?;
        for entry in entries {
            callback(entry.borrowed());
        }
        Ok(())
    }

    fn exists(&self, entry: DirEntry) -> bool {
        match entry {
            DirEntry::File(id, ext) => self.files.contains(&asset_key(id, ext)),
            DirEntry::Directory(id) => self.directories.contains_key(id),
        }
    }
}

#[cfg(all(test, not(target_os = "trueos")))]
mod tests {
    use super::*;
    use assets_manager::{AssetCache, asset::Ron, source::FileSystem};

    #[test]
    fn prepared_database_preserves_catalog_and_loads_after_source_is_deleted() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("common")).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::write(root.join("common/canary.canary"), b"VELOREN_CANARY_MAGIC").unwrap();
        std::fs::write(root.join("nested/value.ron"), b"[4, 8, 15]").unwrap();
        let binary: Vec<_> = (0..150_000).map(|i| (i % 251) as u8).collect();
        std::fs::write(root.join("nested/blob.bin"), &binary).unwrap();
        let source = PicassoSource::import(&FileSystem::new(root).unwrap()).unwrap();
        let original_files = source.files.clone();
        let original_bytes = source.bytes;
        let image = source.into_database_image().unwrap();
        directory.close().unwrap();
        let source = PicassoSource::from_database_image(image).unwrap();
        assert_eq!(source.files, original_files);
        assert_eq!(source.bytes, original_bytes);
        assert_eq!(source.read("nested.blob", "bin").unwrap().as_ref(), binary);
        assert!(source.exists(DirEntry::Directory("empty")));
        source
            .read_dir("empty", &mut |_| panic!("empty directory"))
            .unwrap();
        let cache = AssetCache::with_source(source);
        assert_eq!(
            cache
                .load::<Ron<Vec<u32>>>("nested.value")
                .unwrap()
                .read()
                .0,
            [4, 8, 15]
        );
    }

    #[test]
    fn prepared_database_rejects_missing_canary_and_wrong_catalog_version() {
        for version in [1, 2] {
            let store = Picasso::new().unwrap();
            let catalog = Catalog {
                version,
                directories: BTreeMap::from([(String::new(), Vec::new())]),
                files: BTreeSet::new(),
                bytes: 0,
            };
            store
                .put_embedded_asset(
                    CATALOG_KEY,
                    ron::ser::to_string(&catalog).unwrap().as_bytes(),
                )
                .unwrap();
            let image = store.into_runtime_database_image().unwrap();
            assert!(PicassoSource::from_database_image(image).is_err());
        }
    }

    #[test]
    #[ignore]
    fn prepared_full_tree_round_trip() {
        let image = std::fs::read(
            std::env::var_os("VOXYGEN_TEST_DATABASE").expect("set VOXYGEN_TEST_DATABASE"),
        )
        .unwrap();
        let start = Instant::now();
        let source = PicassoSource::from_database_image(image).unwrap();
        let files = source.files.len();
        let bytes = source.bytes;
        let opened = start.elapsed().as_secs_f64();
        let original = super::super::fs::FileSystem::new().unwrap();
        for entries in source.directories.values() {
            for entry in entries {
                if let Entry::File(id, ext) = entry {
                    assert_eq!(
                        source.read(id, ext).unwrap().as_ref(),
                        original.read(id, ext).unwrap().as_ref(),
                        "{id}.{ext}"
                    );
                }
            }
        }
        let cache = AssetCache::with_source(source);
        cache
            .load::<super::super::Image>("voxygen.background.hurt")
            .unwrap();
        cache
            .load::<super::super::DotVox>("voxygen.voxel.lantern.red-0")
            .unwrap();
        println!(
            "Opened prebuilt Picasso database in {opened:.3}s; verified all {files} assets / {bytes} bytes and PNG/VOX decoders"
        );
    }

    #[test]
    fn serves_bytes_directories_and_decoded_assets_after_tree_is_removed() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::create_dir(root.join("nested")).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        let binary: Vec<u8> = (0..150_000).map(|i| (i % 251) as u8).collect();
        std::fs::write(root.join("nested/blob.bin"), &binary).unwrap();
        std::fs::write(root.join("nested/value.ron"), "[4, 8, 15]").unwrap();
        std::fs::write(root.join("zero.bin"), []).unwrap();
        let source = PicassoSource::import(&FileSystem::new(root).unwrap()).unwrap();
        directory.close().unwrap();

        assert_eq!(source.read("nested.blob", "bin").unwrap().as_ref(), binary);
        assert!(source.read("zero", "bin").unwrap().as_ref().is_empty());
        assert!(source.exists(DirEntry::File("nested.blob", "bin")));
        assert!(source.exists(DirEntry::Directory("empty")));
        assert!(!source.exists(DirEntry::File("missing", "bin")));
        assert_eq!(
            source.read("missing", "bin").unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            source.read_dir("missing", &mut |_| {}).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
        source
            .read_dir("empty", &mut |_| panic!("empty directory"))
            .unwrap();
        let mut files = Vec::new();
        source
            .read_dir("nested", &mut |entry| {
                if let DirEntry::File(id, ext) = entry {
                    files.push((id.to_owned(), ext.to_owned()));
                }
            })
            .unwrap();
        files.sort();
        assert_eq!(
            files,
            [
                ("nested.blob".into(), "bin".into()),
                ("nested.value".into(), "ron".into())
            ]
        );
        let cache = AssetCache::with_source(source);
        assert_eq!(
            cache
                .load::<Ron<Vec<u32>>>("nested.value")
                .unwrap()
                .read()
                .0,
            [4, 8, 15]
        );
    }

    #[test]
    fn excludes_archive_and_checksum_without_excluding_game_lz4_assets() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        std::fs::write(root.join("voxygen-omitted-assets.tar.lz4"), b"bundle").unwrap();
        std::fs::write(
            root.join("voxygen-omitted-assets.tar.lz4.sha256"),
            b"checksum",
        )
        .unwrap();
        std::fs::write(root.join("voxygen-assets.redb"), b"database").unwrap();
        std::fs::write(root.join("voxygen-assets.redb.lz4"), b"database bundle").unwrap();
        std::fs::write(root.join("voxygen-assets.redb.lz4.sha256"), b"checksum").unwrap();
        std::fs::write(root.join("chunk.lz4"), b"game data").unwrap();
        let source = PicassoSource::import(&FileSystem::new(root).unwrap()).unwrap();
        assert_eq!(source.files.len(), 1);
        assert_eq!(source.read("chunk", "lz4").unwrap().as_ref(), b"game data");
        assert!(!source.exists(DirEntry::File("voxygen-omitted-assets", "tar.lz4")));
        assert!(!source.exists(DirEntry::File("voxygen-omitted-assets", "tar.lz4.sha256")));
    }

    /// Run explicitly against the real extracted asset tree on Ubuntu.
    #[test]
    #[ignore]
    fn full_tree_round_trip() {
        let original = super::super::fs::FileSystem::new().unwrap();
        let start = Instant::now();
        let source = PicassoSource::import(&original).unwrap();
        println!(
            "Imported {} assets, {} bytes in {:.2}s",
            source.files.len(),
            source.bytes,
            start.elapsed().as_secs_f64()
        );
        for entries in source.directories.values() {
            for entry in entries {
                if let Entry::File(id, ext) = entry {
                    assert_eq!(
                        source.read(id, ext).unwrap().as_ref(),
                        original.read(id, ext).unwrap().as_ref(),
                        "{id}.{ext}"
                    );
                }
            }
        }
        println!("Every imported asset matches its original bytes");
        let cache = AssetCache::with_source(source);
        // Exercise normal graphical decoders, not just raw byte lookup.
        cache
            .load::<super::super::Image>("voxygen.background.hurt")
            .unwrap();
        cache
            .load::<super::super::DotVox>("voxygen.voxel.lantern.red-0")
            .unwrap();
    }
}
