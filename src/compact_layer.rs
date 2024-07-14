use crate::index::LayerIndexItem;
use byte_unit::Byte;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::path::PathBuf;

#[derive(Debug, Default)]
pub struct CompactLayer {
    pub paths: HashSet<PathBuf>,
    pub total_size: Byte,
}

impl CompactLayer {
    pub fn from_vec(items: Vec<LayerIndexItem>) -> Self {
        let total_size: u64 = items.iter().map(|f| f.compressed_size.as_u64()).sum();
        let paths = items.into_iter().map(|f| f.entry_path).collect();
        CompactLayer {
            paths,
            total_size: Byte::from(total_size),
        }
    }
}

impl Display for CompactLayer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Layer size: {:#.1} paths: {}",
            self.total_size.get_adjusted_unit(byte_unit::Unit::MB),
            self.paths.len()
        )
    }
}
