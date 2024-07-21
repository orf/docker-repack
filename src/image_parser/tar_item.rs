use crate::image_parser::image::LayerID;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::PathBuf;
use tar::{Entry, EntryType};

#[derive(Debug, Clone, Eq)]
pub struct TarItem {
    pub layer_id: LayerID,
    pub path: PathBuf,
    pub raw_size: u64,
    pub is_dir: bool,
    pub is_regular_file: bool,
}

impl Hash for TarItem {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.layer_id.hash(state);
        self.path.hash(state);
    }
}

impl PartialEq for TarItem {
    fn eq(&self, other: &Self) -> bool {
        other.layer_id.eq(&self.layer_id) && other.path.eq(&self.path)
    }
}

impl TarItem {
    pub fn from_entry(layer_index: LayerID, entry: &Entry<impl Read>) -> anyhow::Result<Self> {
        let entry_type = entry.header().entry_type();
        Ok(Self {
            raw_size: entry.size(),
            layer_id: layer_index,
            path: entry.path()?.to_path_buf(),
            is_dir: entry_type == EntryType::Directory,
            is_regular_file: entry_type == EntryType::Regular,
        })
    }

    pub fn is_tiny(&self) -> bool {
        self.raw_size < 512
    }

    pub fn should_attempt_compression(&self) -> bool {
        self.is_regular_file && !self.is_tiny()
    }
}
