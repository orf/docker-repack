use std::cmp::Ordering;
use crate::image_parser::image_reader::SourceLayerID;
use anyhow::bail;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use tar::{Entry, EntryType};

pub type TarItemKey<'a> = (SourceLayerID, &'a PathBuf);

#[derive(Debug, Clone, Eq, PartialEq, strum_macros::Display, Ord, PartialOrd)]
enum FileType {
    Empty,
    NotEmpty([u8; 32]),
}

// The ordering of this enum is important, as it is used to sort TarItem.
// Files must come before HardLinks, as the hardlink target must be present.
#[derive(Debug, Clone, Eq, PartialEq, strum_macros::Display, Ord, PartialOrd)]
enum TarItemType {
    File(FileType),
    HardLink(PathBuf),
    Symlink(PathBuf),
    Directory,
}

#[derive(Debug, Clone, Eq, PartialEq, PartialOrd)]
pub struct TarItem {
    pub layer_id: SourceLayerID,
    pub path: PathBuf,
    pub size: u64,
    type_: TarItemType,
}

impl Ord for TarItem {
    fn cmp(&self, other: &Self) -> Ordering {
        self.type_.cmp(&other.type_)
    }
}

impl TarItem {
    pub fn from_entry(
        layer_index: SourceLayerID,
        mut entry: &mut Entry<impl Read>,
    ) -> anyhow::Result<Self> {
        let entry_size = entry.size();
        let entry_type = entry.header().entry_type();
        let path = entry.path()?.to_path_buf();
        let type_ = match entry_type {
            EntryType::Directory => TarItemType::Directory,
            EntryType::Symlink => match entry.link_name()? {
                None => bail!("Symlink entry without link name: {path:?}"),
                Some(link) => TarItemType::Symlink(link.to_path_buf()),
            }
            EntryType::Link => match entry.link_name()? {
                None => bail!("Link entry without link name: {path:?}"),
                Some(link) => TarItemType::HardLink(link.to_path_buf()),
            },
            EntryType::Regular => {
                if entry_size > 0 {
                    let mut hasher = Sha256::new();
                    std::io::copy(&mut entry, &mut hasher)?;
                    let hash: [u8; 32] = hasher.finalize().into();
                    TarItemType::File(FileType::NotEmpty(hash))
                } else {
                    TarItemType::File(FileType::Empty)
                }
            }
            _ => bail!("Unsupported entry type: {:?}", entry_type),
        };

        Ok(Self {
            size: entry_size,
            layer_id: layer_index,
            path,
            type_,
        })
    }

    pub fn key(&self) -> TarItemKey {
        (self.layer_id, &self.path)
    }

    pub fn key_for_hardlink(&self) -> Option<TarItemKey> {
        match &self.type_ {
            TarItemType::HardLink(path) => Some((self.layer_id, path)),
            _ => None,
        }
    }

    pub fn content_hash(&self) -> Option<[u8; 32]> {
        match self.type_ {
            TarItemType::File(FileType::NotEmpty(hash)) => Some(hash),
            _ => None,
        }
    }

    pub fn link_target(&self) -> Option<&PathBuf> {
        match &self.type_ {
            TarItemType::HardLink(path) | TarItemType::Symlink(path) => Some(path),
            _ => None
        }
    }

    pub fn is_symlink(&self) -> bool {
        matches!(self.type_, TarItemType::Symlink(_))
    }

    pub fn is_dir(&self) -> bool {
        matches!(self.type_, TarItemType::Directory)
    }

    pub fn is_file(&self) -> bool {
        matches!(self.type_, TarItemType::File(_))
    }

    pub fn is_non_empty_file(&self) -> bool {
        matches!(self.type_, TarItemType::File(FileType::NotEmpty(_)))
    }

    pub fn is_tiny_file(&self) -> bool {
        self.is_file() && self.size < 1024
    }
}
