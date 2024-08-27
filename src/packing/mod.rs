mod plan;
pub use compressed_layer_packer::CompressedLayerPacker;
pub use plan::RepackPlan;
// mod simple_layer_packer;
// pub use simple_layer_packer::SimpleLayerPacker;

#[cfg(feature = "split_files")]
mod combiner;
mod compressed_layer_packer;

use crate::io::image::writer::{ImageWriter, NewLayerID};
use crate::tar_item::{TarItem, TarItemKey};
#[cfg(feature = "split_files")]
pub use combiner::FileCombiner;

#[derive(Debug, Default, Clone, Copy, strum_macros::EnumString, strum_macros::Display)]
pub enum RepackType {
    #[default]
    #[strum(serialize = "smart")]
    Smart,
    #[strum(serialize = "basic")]
    Basic,
}

pub trait LayerPacker<'a> {
    fn into_inner(self) -> ImageWriter;

    fn layer_for_item(&mut self, item: &'a TarItem, data: &[u8]) -> anyhow::Result<NewLayerID>;

    fn layer_for(
        &mut self,
        key: TarItemKey<'a>,
        size: u64,
        data: &[u8],
        hash: Option<[u8; 32]>,
        hardlink: Option<TarItemKey>,
    ) -> anyhow::Result<NewLayerID>;
}
