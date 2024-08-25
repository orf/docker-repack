use crate::io::image::writer::{ImageWriter, NewLayerID};
use crate::io::TrackedEncoderWriter;
use crate::packing::LayerPacker;
use crate::tar_item::{TarItem, TarItemKey};
use tracing::info;

pub struct CompressedLayerPacker<'a> {
    image_writer: ImageWriter,
    target_size: u64,
    layer_id: NewLayerID,
    encoder: TrackedEncoderWriter<'a>,
}

impl<'a> CompressedLayerPacker<'a> {
    pub fn new(mut image_writer: ImageWriter, target_size: u64) -> anyhow::Result<Self> {
        let layer_id = image_writer.create_new_layer("files")?;
        Ok(Self {
            image_writer,
            target_size,
            layer_id,
            encoder: TrackedEncoderWriter::new()?,
        })
    }

    fn size_remaining(&self) -> u64 {
        self.target_size.saturating_sub(self.encoder.bytes_written())
    }

    fn has_reached_target_size(&self) -> bool {
        self.size_remaining() == 0
    }

    fn set_new_encoder(&mut self) -> anyhow::Result<()> {
        self.encoder.flush()?;
        info!(
            "Finished layer {} - written={:#.1}",
            self.layer_id,
            crate::utils::display_bytes(self.encoder.bytes_written())
        );
        let new_layer_id = self.image_writer.create_new_layer("files")?;
        self.layer_id = new_layer_id;
        self.encoder = TrackedEncoderWriter::new()?;
        Ok(())
    }
}

impl<'a> LayerPacker<'a> for CompressedLayerPacker<'a> {
    fn into_inner(self) -> ImageWriter {
        self.image_writer
    }

    fn layer_for_item(&mut self, _item: &'a TarItem, data: &[u8]) -> anyhow::Result<NewLayerID> {
        let current_layer_id = self.layer_id;

        self.encoder.copy_item(data)?;

        if !self.has_reached_target_size() {
            return Ok(current_layer_id);
        }

        self.set_new_encoder()?;

        Ok(current_layer_id)
    }

    fn layer_for(
        &mut self,
        _key: TarItemKey<'a>,
        _size: u64,
        _hash: Option<[u8; 32]>,
        _hardlink: Option<TarItemKey>,
    ) -> NewLayerID {
        #[cfg(feature = "split_files")]
        todo!();
        #[cfg(not(feature = "split_files"))]
        unimplemented!("Not implemented for non-split files builds");
    }
}
