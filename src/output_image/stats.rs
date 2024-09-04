use crate::input::Platform;
use crate::output_image::image::WrittenLayer;
use crate::output_image::layers::LayerType;
use crate::progress::display_bytes;
use std::fmt::Display;

pub struct WrittenLayerStats {
    pub type_: LayerType,
    pub compressed_file_size: u64,
    pub raw_file_size: u64,
    pub item_count: usize,
}

impl WrittenLayerStats {
    pub fn from_written_layer(layer: &WrittenLayer) -> Self {
        Self {
            type_: layer.layer.type_,
            compressed_file_size: layer.compressed_file_size,
            raw_file_size: layer.layer.raw_size(),
            item_count: layer.layer.len(),
        }
    }
}

impl Display for WrittenLayerStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Type: {}, Size: {:#.1}, Uncompressed Size: {:#.1}, File Count: {}",
            self.type_,
            display_bytes(self.compressed_file_size),
            display_bytes(self.raw_file_size),
            self.item_count
        )
    }
}

pub struct WrittenImageStats {
    pub layers: Vec<WrittenLayerStats>,
    pub platform: Platform,
}

impl WrittenImageStats {
    pub fn new(layers: &[WrittenLayer], platform: Platform) -> Self {
        Self {
            platform,
            layers: layers.iter().map(WrittenLayerStats::from_written_layer).collect(),
        }
    }

    pub fn description(&self) -> String {
        let total_raw_size = self.layers.iter().map(|l| l.raw_file_size).sum::<u64>();
        let total_compressed_size = self.layers.iter().map(|l| l.compressed_file_size).sum::<u64>();
        format!(
            "# Repacked image - {}\n\n- layers: {}\n- raw size: {}\n- compressed size: {}",
            self.platform,
            self.layers.len(),
            display_bytes(total_raw_size),
            display_bytes(total_compressed_size)
        )
    }
}
