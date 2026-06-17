//! Thin adapter over `rpf_archive`, adapted from VIRUXE/rpf-cli (MIT) `src/rpf.rs`.
//! Loads an archive fully into memory and exposes list/find/extract, plus
//! descent into nested RPFs (x64e.rpf -> levels/gta5/vehicles.rpf).

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub use rpf_archive::{
    build_directory_tree, list_all_files, DirNode, FileRef, GtaKeys, RpfArchive, RpfEncryption,
    RpfEntryKind,
};

pub struct Archive {
    pub name: String,
    pub encryption: RpfEncryption,
    pub entry_count: usize,
    pub dir_count: usize,
    pub root: DirNode,
    archive: RpfArchive,
    data: Vec<u8>,
}

impl Archive {
    pub fn open(path: &Path, keys: Option<&GtaKeys>) -> Result<Self> {
        let data = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        Self::from_bytes(data, &name, keys)
    }

    pub fn from_bytes(data: Vec<u8>, name: &str, keys: Option<&GtaKeys>) -> Result<Self> {
        let archive = RpfArchive::parse(&data, name, keys)
            .with_context(|| format!("parsing RPF '{name}'"))?;
        let encryption = archive.encryption;
        let entry_count = archive.entries.len();
        let dir_count = archive.entries.iter().filter(|e| e.is_directory()).count();
        let root = build_directory_tree(&archive.entries);
        Ok(Self {
            name: name.to_string(),
            encryption,
            entry_count,
            dir_count,
            root,
            archive,
            data,
        })
    }

    pub fn list_files(&self) -> Vec<&FileRef> {
        list_all_files(&self.root)
    }

    /// Find by full path or bare filename (case-insensitive).
    pub fn find_file(&self, path: &str) -> Option<&FileRef> {
        let needle = path.replace('\\', "/").to_lowercase();
        find_in_dir(&self.root, &needle)
    }

    pub fn extract(&self, file: &FileRef, keys: Option<&GtaKeys>) -> Result<Vec<u8>> {
        let entry = &self.archive.entries[file.entry_index];
        self.archive.extract_entry(&self.data, entry, keys)
    }

    pub fn entry_kind(&self, file: &FileRef) -> &RpfEntryKind {
        &self.archive.entries[file.entry_index].kind
    }

    /// Open a nested RPF found by name inside this archive (e.g. "vehicles.rpf").
    pub fn open_nested(&self, name: &str, keys: Option<&GtaKeys>) -> Result<Archive> {
        let file = self
            .find_file(name)
            .with_context(|| format!("'{name}' not found inside '{}'", self.name))?;
        let bytes = self
            .extract(file, keys)
            .with_context(|| format!("extracting nested '{name}'"))?;
        Archive::from_bytes(bytes, name, keys)
    }
}

fn find_in_dir<'a>(dir: &'a DirNode, path: &str) -> Option<&'a FileRef> {
    for f in &dir.files {
        if f.path.to_lowercase() == path || f.name.to_lowercase() == path {
            return Some(f);
        }
    }
    for sub in &dir.subdirs {
        if let Some(f) = find_in_dir(sub, path) {
            return Some(f);
        }
    }
    None
}

/// Resolve where the keys live. We default to the folder the user pointed us at.
pub fn default_keys_dir() -> PathBuf {
    PathBuf::from(r"C:\Users\nic10\Downloads\archive")
}
