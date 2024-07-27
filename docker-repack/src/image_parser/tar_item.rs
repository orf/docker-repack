use crate::image_parser::image_reader::SourceLayerID;
use crate::image_parser::utils::byte_range_chunks;
use anyhow::bail;
use byte_unit::{Byte, UnitType};
use const_hex::Buffer;
use itertools::Itertools;
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::fmt::{Display, Formatter};
use std::io::Read;
use std::ops::Range;
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

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct TarItem {
    pub layer_id: SourceLayerID,
    pub path: PathBuf,
    pub size: u64,
    type_: TarItemType,
}

impl Display for TarItem {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "TarItem layer_id={:?} path={:?} size={:#.1} type={}",
            self.layer_id,
            self.path,
            Byte::from(self.size).get_appropriate_unit(UnitType::Decimal),
            self.type_
        )
    }
}

impl PartialOrd for TarItem {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
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

    pub fn is_non_empty_file(&self) -> bool {
        matches!(self.type_, TarItemType::File(FileType::NotEmpty(_)))
    }

    pub fn is_tiny_file(&self) -> bool {
        self.is_file() && self.size < 1024
    }

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

#[derive(Debug, Eq, PartialEq)]
pub struct TarItemChunk<'a> {
    pub tar_item: &'a TarItem,
    pub index: usize,
    pub byte_range: Range<u64>,
}

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
}

impl Ord for TarItemChunk<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.tar_item, self.index).cmp(&(other.tar_item, other.index))
    }
}

impl PartialOrd for TarItemChunk<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
