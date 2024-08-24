use crate::io::image::writer::{ImageWriter, NewLayerID};
use crate::tar_item::{TarItem, TarItemKey};
use crate::utils::display_bytes;
use std::collections::HashSet;
use tracing::trace;

pub struct LayerBin<'a> {
    id: NewLayerID,
    total_size: u64,
    items: HashSet<TarItemKey<'a>>,
    hashes: HashSet<[u8; 32]>,
}

impl<'a> LayerBin<'a> {
    pub fn new(id: NewLayerID) -> Self {
        Self {
            id,
            total_size: 0,
            items: HashSet::new(),
            hashes: HashSet::new(),
        }
    }
    pub fn contains_hash(&self, hash: [u8; 32]) -> bool {
        self.hashes.contains(&hash)
    }
    pub fn contains_hardlink_target(&self, key: TarItemKey) -> bool {
        self.items.contains(&key)
    }

    pub fn can_fit_size(&self, target_size: u64, size: u64) -> bool {
        trace!(
            "total_size={} + item_size={} <= {}",
            display_bytes(target_size),
            display_bytes(self.total_size),
            display_bytes(size)
        );
        (self.total_size + size) <= target_size
    }

    pub fn add_item(&mut self, key: TarItemKey<'a>, size: u64, hash: Option<[u8; 32]>) {
        self.items.insert(key);
        if let Some(hash) = hash {
            if !self.hashes.insert(hash) {
                // Don't update total size if the hash was
                // already present
                return;
            }
        }
        self.total_size += size;
    }
}

pub struct SimpleLayerPacker<'a> {
    image_writer: ImageWriter,
    layer_bins: Vec<LayerBin<'a>>,
    target_size: u64,
}

impl<'a> SimpleLayerPacker<'a> {
    pub fn new(image_writer: ImageWriter, target_size: u64) -> SimpleLayerPacker<'a> {
        Self {
            target_size,
            image_writer,
            layer_bins: Vec::new(),
        }
    }

    pub fn into_inner(self) -> ImageWriter {
        self.image_writer
    }

    pub fn layer_for(
        &mut self,
        key: TarItemKey<'a>,
        size: u64,
        hash: Option<[u8; 32]>,
        hardlink: Option<TarItemKey>,
    ) -> NewLayerID {
        if let Some(hash) = hash {
            for layer_bin in self.layer_bins.iter_mut() {
                if layer_bin.contains_hash(hash) {
                    layer_bin.add_item(key, size, Some(hash));
                    return layer_bin.id;
                }
            }
        }

        if let Some(hardlink) = hardlink {
            for layer_bin in self.layer_bins.iter_mut() {
                if layer_bin.contains_hardlink_target(hardlink) {
                    layer_bin.add_item(key, size, hash);
                    return layer_bin.id;
                }
            }
        }

        for layer_bin in self.layer_bins.iter_mut() {
            if layer_bin.can_fit_size(self.target_size, size) {
                layer_bin.add_item(key, size, hash);
                return layer_bin.id;
            }
        }
        let new_layer_id = self.image_writer.create_new_layer("files").unwrap();
        let mut new_layer_bin = LayerBin::new(new_layer_id);
        new_layer_bin.add_item(key, size, hash);
        self.layer_bins.push(new_layer_bin);
        new_layer_id
    }

    pub fn layer_for_item(&mut self, item: &'a TarItem) -> NewLayerID {
        let key = item.key();
        let hash = item.content_hash();
        let hardlink_target = item.key_for_hardlink();
        self.layer_for(key, item.size, hash, hardlink_target)
    }
}
