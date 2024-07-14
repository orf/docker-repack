use byte_unit::Byte;
use std::io;
use std::path::PathBuf;
use tar::EntryType;
use thiserror::Error;
use crate::compression;

#[derive(Error, Debug)]
pub enum EntryError {
    #[error("Unsupported tar file item `{0:?}`")]
    InvalidEntryType(EntryType),
    #[error("Error compressing tar entry `{0:?}`")]
    CompressionError(#[from] io::Error),
}

#[derive(Debug, Clone)]
pub struct LayerIndexItem {
    pub entry_path: PathBuf,
    pub entry_type: EntryType,
    pub size: Byte,
    pub compressed_size: Byte,
}

impl LayerIndexItem {
    pub fn from_header(
        path: PathBuf,
        entry_type: EntryType,
        data: Vec<u8>,
        compression_buffer: &mut Vec<u8>,
    ) -> Result<Self, EntryError> {
        let compressed_size = compression::calculate_compressed_size_gzip(&data, compression_buffer)?;
        Ok(Self {
            entry_path: path,
            entry_type,
            size: Byte::from(data.len() as u64),
            compressed_size,
        })
    }

    pub fn is_not_dir(&self) -> bool {
        self.entry_type != EntryType::Directory
    }
}
