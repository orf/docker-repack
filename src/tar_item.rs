use crate::io::image::reader::SourceLayerID;
use crate::utils::display_bytes;
use anyhow::bail;

use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};
use std::io::{BufRead, Seek};
use std::path::PathBuf;
use tar::{Entry, EntryType};

#[cfg(feature = "split_files")]
use crate::utils::byte_range_chunks;
#[cfg(feature = "split_files")]
use const_hex::Buffer;
#[cfg(feature = "split_files")]
use itertools::Itertools;
#[cfg(feature = "split_files")]
use std::ops::Range;

pub type TarItemKey<'a> = (SourceLayerID, &'a PathBuf);
pub type TarItemSortKey = TarItemType;

#[derive(Debug, Clone, Eq, PartialEq, strum_macros::Display, Ord, PartialOrd)]
pub enum FileType {
    Empty,
    NotEmpty([u8; 32]),
}

// The ordering of this enum is important, as it is used to sort TarItem.
// Files must come before HardLinks, as the hardlink target must be present.
#[derive(Debug, Clone, Eq, PartialEq, strum_macros::Display, Ord, PartialOrd)]
pub enum TarItemType {
    Directory,
    Symlink(PathBuf),
    File(FileType),
    HardLink(PathBuf),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TarItem {
    pub layer_id: SourceLayerID,
    pub path: PathBuf,
    pub size: u64,
    pub header_position: u64,
    pub data_position: u64,
    type_: TarItemType,
}

impl Display for TarItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TarItem layer_id={:?} path={:?} size={:#.1} type={}",
            self.layer_id,
            self.path,
            display_bytes(self.size),
            self.type_
        )
    }
}

impl TarItem {
    pub fn from_entry(layer_index: SourceLayerID, mut entry: &mut Entry<impl BufRead + Seek>) -> anyhow::Result<Self> {
        let entry_size = entry.size();
        let entry_type = entry.header().entry_type();
        let header_position = entry.raw_header_position();
        let data_position = entry.raw_file_position();
        let path = entry.path()?.to_path_buf();
        let type_ = match entry_type {
            EntryType::Directory => TarItemType::Directory,
            EntryType::Symlink => match entry.link_name()? {
                None => bail!("Symlink entry without link name: {path:?}"),
                Some(link) => TarItemType::Symlink(link.to_path_buf()),
            },
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
            header_position,
            data_position,
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

    pub fn sort_key(&self) -> TarItemSortKey {
        self.type_.clone()
    }

    #[cfg(feature = "split_files")]
    pub fn content_hash_hex(&self) -> Option<Buffer<32>> {
        match self.content_hash() {
            None => None,
            Some(hash) => {
                let mut buffer = Buffer::<32>::new();
                buffer.format(&hash);
                Some(buffer)
            }
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
            _ => None,
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

    pub fn is_tiny_file(&self) -> bool {
        self.is_file() && self.size < 1024
    }

    #[cfg(feature = "split_files")]
    pub fn split_into_chunks(&self, chunk_size: u64) -> Vec<TarItemChunk> {
        byte_range_chunks(self.size, chunk_size)
            .enumerate()
            .map(move |(idx, byte_range)| TarItemChunk {
                tar_item: self,
                index: idx,
                byte_range,
            })
            .collect_vec()
    }
}

#[cfg(feature = "split_files")]
#[derive(Debug, Eq, PartialEq)]
pub struct TarItemChunk<'a> {
    pub tar_item: &'a TarItem,
    pub index: usize,
    pub byte_range: Range<u64>,
}

#[cfg(feature = "split_files")]
impl TarItemChunk<'_> {
    pub fn dest_path(&self) -> PathBuf {
        let file_name = self.tar_item.path.file_name().unwrap();
        self.tar_item.path.with_file_name(format!(
            ".repack._split-{}-{}-{}",
            file_name.to_str().unwrap(),
            self.byte_range.start,
            self.byte_range.end
        ))
    }

    pub fn size(&self) -> u64 {
        self.byte_range.end - self.byte_range.start
    }
}
