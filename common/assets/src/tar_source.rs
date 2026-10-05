//! Virtual read-only asset files borrowed from an unpacked TAR in RAM.

use super::picasso_source::Entry;
use assets_manager::source::{DirEntry, FileContent, Source};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    ops::Range,
};

const MAX_FILES: usize = 16_384;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
pub(super) const MAX_TAR_BYTES: usize = MAX_TOTAL_BYTES as usize + MAX_FILES * 4096 + 1024;

pub(super) struct TarSource<'a> {
    bytes: &'a [u8],
    files: BTreeMap<(String, String), Range<usize>>,
    directories: BTreeMap<String, BTreeSet<Entry>>,
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

impl<'a> TarSource<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> io::Result<Self> {
        if bytes.len() > MAX_TAR_BYTES {
            return Err(invalid("TAR exceeds asset size limit"));
        }
        let mut result = Self {
            bytes,
            files: BTreeMap::new(),
            directories: BTreeMap::new(),
        };
        result.directories.insert(String::new(), BTreeSet::new());
        let mut archive = tar::Archive::new(io::Cursor::new(bytes));
        let mut total = 0u64;
        for entry in archive.entries()? {
            let entry = entry?;
            let kind = entry.header().entry_type();
            if !kind.is_file() && !kind.is_dir() {
                return Err(invalid("asset TAR contains a link or unsupported member"));
            }
            let path_bytes = entry.path_bytes();
            let path =
                std::str::from_utf8(&path_bytes).map_err(|_| invalid("non-UTF8 TAR asset path"))?;
            let path = path
                .strip_prefix("./")
                .unwrap_or(path)
                .trim_end_matches('/');
            let path = path.strip_prefix("assets/").unwrap_or(path);
            if kind.is_dir() && (path.is_empty() || path == "." || path == "assets") {
                continue;
            }
            let mut components: Vec<&str> = path.split('/').collect();
            if components
                .iter()
                .any(|c| c.is_empty() || *c == "." || *c == ".." || c.contains(['\\', '\0']))
            {
                return Err(invalid(format!("invalid TAR asset path: {path}")));
            }
            if components.len() > 64 || path.len() > 1024 {
                return Err(invalid("TAR asset path exceeds limit"));
            }
            // Match assets_manager's filesystem source: hidden entries are absent.
            if components.iter().any(|c| c.starts_with('.')) {
                continue;
            }
            let filename = if kind.is_file() {
                components.pop()
            } else {
                None
            };
            let mut parent = String::new();
            for component in components {
                if component.contains('.') {
                    return Err(invalid("asset directory name contains a dot"));
                }
                let id = if parent.is_empty() {
                    component.to_owned()
                } else {
                    format!("{parent}.{component}")
                };
                result
                    .directories
                    .entry(parent)
                    .or_default()
                    .insert(Entry::Directory(id.clone()));
                result.directories.entry(id.clone()).or_default();
                parent = id;
            }
            if let Some(filename) = filename {
                let (stem, ext) = filename.split_once('.').unwrap_or((filename, ""));
                let id = if parent.is_empty() {
                    stem.to_owned()
                } else {
                    format!("{parent}.{stem}")
                };
                if ext.is_empty() && result.directories.contains_key(&id) {
                    return Err(invalid("TAR file/directory conflict"));
                }
                let size = entry.size();
                total = total
                    .checked_add(size)
                    .ok_or_else(|| invalid("TAR byte count overflow"))?;
                if size > MAX_FILE_BYTES
                    || total > MAX_TOTAL_BYTES
                    || result.files.len() >= MAX_FILES
                {
                    return Err(invalid("TAR asset count or byte limit exceeded"));
                }
                let start = usize::try_from(entry.raw_file_position())
                    .map_err(|_| invalid("TAR offset overflow"))?;
                let end = start
                    .checked_add(size as usize)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(|| invalid("truncated TAR asset"))?;
                if result
                    .files
                    .insert((id.clone(), ext.to_owned()), start..end)
                    .is_some()
                {
                    return Err(invalid(format!("duplicate TAR asset: {id}.{ext}")));
                }
                result
                    .directories
                    .entry(parent)
                    .or_default()
                    .insert(Entry::File(id, ext.to_owned()));
            }
        }
        if result.files.is_empty() {
            return Err(invalid("asset TAR contains no files"));
        }
        Ok(result)
    }

    pub(super) fn file_count(&self) -> usize {
        self.files.len()
    }

    pub(super) fn validate_canary(&self) -> io::Result<()> {
        let canary = self.read("common.canary", "canary")?;
        if !canary.as_ref().starts_with(b"VELOREN_CANARY_MAGIC") {
            return Err(invalid("asset TAR contains an invalid Veloren canary"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::picasso_source::PicassoSource;

    fn bundle(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (path, bytes) in files {
            let mut header = tar::Header::new_ustar();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *bytes).unwrap();
        }
        builder.into_inner().unwrap()
    }

    #[test]
    fn tar_virtual_files_feed_picasso_without_retaining_the_archive() {
        let large: Vec<u8> = (0..150_000).map(|i| (i % 251) as u8).collect();
        let bytes = bundle(&[
            ("assets/common/canary.canary", b"VELOREN_CANARY_MAGIC"),
            ("assets/voxygen/settings.ron", b"[4, 8, 15]"),
            ("assets/voxygen/model.bin", &large),
        ]);
        let tar = TarSource::new(&bytes).unwrap();
        tar.validate_canary().unwrap();
        assert_eq!(tar.file_count(), 3);
        let db = PicassoSource::import(&tar).unwrap();
        drop(tar);
        drop(bytes);
        assert_eq!(db.read("voxygen.model", "bin").unwrap().as_ref(), large);
        let cache = assets_manager::AssetCache::with_source(db);
        assert_eq!(
            cache
                .load::<assets_manager::asset::Ron<Vec<u32>>>("voxygen.settings")
                .unwrap()
                .read()
                .0,
            [4, 8, 15]
        );
    }

    #[test]
    fn omitted_common_is_imported_without_loading_loose_graphics() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("common")).unwrap();
        std::fs::create_dir(directory.path().join("voxygen")).unwrap();
        std::fs::write(
            directory.path().join("common/canary.canary"),
            b"VELOREN_CANARY_MAGIC",
        )
        .unwrap();
        std::fs::write(directory.path().join("common/item.ron"), b"[1]").unwrap();
        std::fs::write(
            directory.path().join("voxygen/loose.ron"),
            b"must not be loaded",
        )
        .unwrap();
        let common = assets_manager::source::FileSystem::new(directory.path()).unwrap();
        let bytes = bundle(&[("assets/voxygen/picture.png", b"archive bytes")]);
        let tar = TarSource::new(&bytes).unwrap();
        let db = PicassoSource::import(&ArchiveWithCommon {
            archive: &tar,
            common: &common,
        })
        .unwrap();
        directory.close().unwrap();
        assert_eq!(db.read("common.item", "ron").unwrap().as_ref(), b"[1]");
        assert_eq!(
            db.read("voxygen.picture", "png").unwrap().as_ref(),
            b"archive bytes"
        );
        assert!(!db.exists(DirEntry::File("voxygen.loose", "ron")));
    }

    #[test]
    fn duplicate_and_truncated_archives_are_rejected() {
        let bytes = bundle(&[
            ("assets/common/item.ron", b"a"),
            ("assets/common/item.ron", b"b"),
        ]);
        assert!(TarSource::new(&bytes).is_err());
        let bytes = bundle(&[("assets/common/item.ron", b"a")]);
        assert!(TarSource::new(&bytes[..512]).is_err());
    }

    #[test]
    #[ignore]
    fn full_archive_round_trip() {
        let bytes = std::fs::read(std::env::var("VOXYGEN_TEST_TAR").expect("set VOXYGEN_TEST_TAR"))
            .unwrap();
        let tar = TarSource::new(&bytes).unwrap();
        let original = assets_manager::source::FileSystem::new(&*crate::ASSETS_PATH).unwrap();
        let start = std::time::Instant::now();
        let db = if tar.exists(DirEntry::File("common.canary", "canary")) {
            tar.validate_canary().unwrap();
            PicassoSource::import(&tar).unwrap()
        } else {
            PicassoSource::import(&ArchiveWithCommon {
                archive: &tar,
                common: &original,
            })
            .unwrap()
        };
        for (id, ext) in tar.files.keys() {
            assert_eq!(
                db.read(id, ext).unwrap().as_ref(),
                original.read(id, ext).unwrap().as_ref(),
                "{id}.{ext}"
            );
        }
        println!(
            "Verified {} TAR assets imported into Picasso in {:.2}s",
            tar.file_count(),
            start.elapsed().as_secs_f64()
        );
        drop(tar);
        drop(bytes);
        let cache = assets_manager::AssetCache::with_source(db);
        cache
            .load::<crate::Image>("voxygen.background.hurt")
            .unwrap();
        cache
            .load::<crate::DotVox>("voxygen.voxel.lantern.red-0")
            .unwrap();
    }
}

impl Source for TarSource<'_> {
    fn read(&self, id: &str, ext: &str) -> io::Result<FileContent<'_>> {
        let range = self
            .files
            .get(&(id.to_owned(), ext.to_owned()))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, format!("TAR asset {id}.{ext}"))
            })?;
        Ok(FileContent::Slice(&self.bytes[range.clone()]))
    }
    fn read_dir(&self, id: &str, callback: &mut dyn FnMut(DirEntry)) -> io::Result<()> {
        let entries = self.directories.get(id).ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, format!("TAR directory {id}"))
        })?;
        for entry in entries {
            callback(entry.borrowed());
        }
        Ok(())
    }
    fn exists(&self, entry: DirEntry) -> bool {
        match entry {
            DirEntry::File(id, ext) => self.files.contains_key(&(id.to_owned(), ext.to_owned())),
            DirEntry::Directory(id) => self.directories.contains_key(id),
        }
    }
}

/// The old seed bundle omitted common. Read only that subtree from loose assets
/// during ingestion, with archive entries taking priority.
pub(super) struct ArchiveWithCommon<'a, S> {
    pub archive: &'a TarSource<'a>,
    pub common: &'a S,
}
fn is_common(id: &str) -> bool {
    id == "common" || id.starts_with("common.")
}
impl<S: Source> Source for ArchiveWithCommon<'_, S> {
    fn read(&self, id: &str, ext: &str) -> io::Result<FileContent<'_>> {
        if !self.archive.exists(DirEntry::File(id, ext)) && is_common(id) {
            self.common.read(id, ext)
        } else {
            self.archive.read(id, ext)
        }
    }
    fn read_dir(&self, id: &str, callback: &mut dyn FnMut(DirEntry)) -> io::Result<()> {
        let mut entries = BTreeSet::new();
        let mut collect = |entry: DirEntry| {
            entries.insert(match entry {
                DirEntry::File(id, ext) => Entry::File(id.to_owned(), ext.to_owned()),
                DirEntry::Directory(id) => Entry::Directory(id.to_owned()),
            });
        };
        let archive_result = self.archive.read_dir(id, &mut collect);
        if is_common(id) {
            if let Err(error) = self.common.read_dir(id, &mut collect) {
                if error.kind() != io::ErrorKind::NotFound || archive_result.is_err() {
                    return Err(error);
                }
            }
        } else if id.is_empty() {
            entries.insert(Entry::Directory("common".into()));
        } else {
            archive_result?;
        }
        for entry in entries {
            callback(entry.borrowed());
        }
        Ok(())
    }
    fn exists(&self, entry: DirEntry) -> bool {
        self.archive.exists(entry) || (is_common(entry.id()) && self.common.exists(entry))
    }
}
