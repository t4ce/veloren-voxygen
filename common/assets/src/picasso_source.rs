//! One-time filesystem ingestion, followed by a RAM-only asset source.
//! Raw bytes belong to Picasso; assets_manager still caches decoded objects.

use assets_manager::source::{DirEntry, FileContent, Source};
use picasso::Picasso;
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

#[derive(Debug)]
enum Entry {
    File(String, String),
    Directory(String),
}

impl Entry {
    fn borrowed(&self) -> DirEntry<'_> {
        match self {
            Self::File(id, ext) => DirEntry::File(id, ext),
            Self::Directory(id) => DirEntry::Directory(id),
        }
    }
}

fn asset_key(id: &str, ext: &str) -> String {
    // Asset IDs and extensions cannot contain a path separator.
    format!("{id}/{ext}")
}

fn is_bundle(id: &str, ext: &str) -> bool {
    ext == "tar.lz4"
        || ext == "tar.lz4.sha256"
        || (ext == "lz4" && id.ends_with(".tar"))
        || (ext == "sha256" && id.ends_with(".tar.lz4"))
}

impl PicassoSource {
    pub(super) fn new() -> io::Result<Self> {
        let start = Instant::now();
        tracing::info!("Importing assets into Picasso's RAM database");
        // Preserve canary validation and override precedence at ingestion.
        // Neither filesystem source nor a path is retained by PicassoSource.
        let source = Self::import(&super::fs::FileSystem::new()?)?;
        tracing::info!(
            files = source.files.len(),
            bytes = source.bytes,
            seconds = start.elapsed().as_secs_f64(),
            "Picasso asset import complete; serving assets from RAM"
        );
        Ok(source)
    }

    pub(super) fn import(source: &impl Source) -> io::Result<Self> {
        let mut result = Self {
            store: Picasso::new().map_err(storage_error)?,
            directories: BTreeMap::new(),
            files: BTreeSet::new(),
            bytes: 0,
        };
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
                    }
                }
            }
            result.directories.insert(directory, entries);
        }
        Ok(result)
    }
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
