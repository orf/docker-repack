use crate::io::image::writer::{ImageWriter, NewLayerID};
use crate::io::TrackedEncoderWriter;
use crate::packing::LayerPacker;
use crate::tar_item::{TarItem, TarItemKey};
use anyhow::bail;
use std::collections::HashMap;
use tracing::info;

pub struct CompressedLayerPacker<'a> {
    image_writer: ImageWriter,
    target_size: u64,
    layer_id: NewLayerID,
    encoder: TrackedEncoderWriter<'a>,
    item_map: HashMap<TarItemKey<'a>, NewLayerID>,
    item_content_map: HashMap<[u8; 32], NewLayerID>,
}

impl<'a> CompressedLayerPacker<'a> {
    pub fn new(mut image_writer: ImageWriter, target_size: u64) -> anyhow::Result<Self> {
        let layer_id = image_writer.create_new_layer("files")?;
        Ok(Self {
            image_writer,
            target_size,
            layer_id,
            encoder: TrackedEncoderWriter::new()?,
            item_map: Default::default(),
            item_content_map: Default::default(),
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

    fn layer_for_item(&mut self, item: &'a TarItem, data: &[u8]) -> anyhow::Result<NewLayerID> {
        let key = item.key();
        let hardlink_key = item.key_for_hardlink();

        let layer_id = self.layer_for(key, item.size, data, item.content_hash(), hardlink_key)?;

        self.item_map.insert(key, layer_id);

        Ok(layer_id)
    }

    fn layer_for(
        &mut self,
        key: TarItemKey<'a>,
        _size: u64,
        data: &[u8],
        hash: Option<[u8; 32]>,
        hardlink: Option<TarItemKey>,
    ) -> anyhow::Result<NewLayerID> {
        if let Some(hardlink) = hardlink {
            match self.item_map.get(&hardlink) {
                Some(layer_id) => return Ok(*layer_id),
                None => {
                    bail!("Hardlink target {hardlink:?} not found for {key:?}");
                }
            }
        }

        if let Some(hash) = hash {
            if let Some(layer_id) = self.item_content_map.get(&hash) {
                return Ok(*layer_id);
            }
        }

        let current_layer_id = self.layer_id;

        self.encoder.copy_item(data)?;

        if !self.has_reached_target_size() {
            return Ok(current_layer_id);
        }

        self.set_new_encoder()?;

        Ok(current_layer_id)
    }
}
