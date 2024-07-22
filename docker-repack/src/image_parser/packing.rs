use crate::image_parser::image_reader::SourceLayerID;
use crate::image_parser::{ImageWriter, TarItem, TarItemKey};
use anyhow::bail;
use byte_unit::{Byte, UnitType};
use itertools::Itertools;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

#[derive(Debug)]
pub struct Bin<'a, 'b> {
    target_size: u64,
    effective_size: u64,
    total_size: u64,
    items: HashMap<TarItemKey<'b>, &'a TarItem>,
    hashes: HashSet<[u8; 32]>,
}

impl<'a: 'b, 'b> Bin<'a, 'b> {
    pub fn new(target_size: u64, item: &'a TarItem) -> Self {
        let hashes = if let Some(hash) = item.content_hash() {
            HashSet::from([hash])
        } else {
            HashSet::new()
        };
        Self {
            target_size,
            effective_size: item.size,
            total_size: item.size,
            items: HashMap::from([(item.key(), item)]),
            hashes,
        }
    }
    pub fn contains_hash(&self, hash: &[u8; 32]) -> bool {
        self.hashes.contains(hash)
    }
    pub fn contains_hardlink_target(&self, key: TarItemKey) -> bool {
        self.items.contains_key(&key)
    }

    pub fn can_fit_item(&self, item: &'a TarItem) -> bool {
        (self.effective_size + item.size) <= self.target_size
    }
    pub fn add_item(&mut self, item: &'a TarItem) -> anyhow::Result<()> {
        if let Some(_) = self.items.insert(item.key(), item) {
            bail!("Item {:?} already present in bin", item.key());
        }
        self.total_size += item.size;
        if let Some(hash) = item.content_hash() {
            if !self.hashes.insert(hash) {
                // Don't update effective size if the hash was
                // already present
                return Ok(());
            }
        }
        self.effective_size += item.size;
        Ok(())
    }

    pub fn into_iter(self) -> impl Iterator<Item = (SourceLayerID, &'a str)> {
        let items = self
            .items
            .values()
            .into_iter()
            .map(|item| (item.layer_id, item.path.to_str().unwrap()))
            .collect_vec();
        items.into_iter()
    }
}

impl Display for Bin<'_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let effective_size =
            Byte::from(self.effective_size).get_appropriate_unit(UnitType::Decimal);
        let total_size = Byte::from(self.total_size).get_appropriate_unit(UnitType::Decimal);
        write!(
            f,
            "Bin: {:>4} items, {:>4} unique items, total size: {:#>10.1} effective size: {:#>10.1}",
            self.items.len(),
            self.hashes.len(),
            total_size,
            effective_size
        )
    }
}

#[derive(Debug)]
pub struct LayerPacker<'a, 'b> {
    name: &'static str,
    target_size: u64,
    bins: Vec<Bin<'a, 'b>>,
}

impl<'a: 'b, 'b> LayerPacker<'a, 'b> {
    pub fn new(name: &'static str, target_size: u64) -> Self {
        LayerPacker {
            name,
            target_size,
            bins: Vec::new(),
        }
    }

    pub fn add_items(&mut self, items: impl Iterator<Item = &'a TarItem>) -> anyhow::Result<()> {
        for item in items.sorted() {
            if let Some(key) = item.key_for_hardlink() {
                if let Some(bin) = self.bins.iter_mut().find(|bin| bin.contains_hardlink_target(key))
                {
                    bin.add_item(item)?;
                    continue;
                }
            }
            if let Some(hash) = item.content_hash() {
                if let Some(bin) = self.bins.iter_mut().find(|bin| bin.contains_hash(&hash)) {
                    bin.add_item(item)?;
                    continue;
                }
            }
            match self.bins.iter_mut().find(|bin| bin.can_fit_item(item)) {
                None => {
                    self.bins.push(Bin::new(self.target_size, item));
                }
                Some(bin) => bin.add_item(item)?,
            }
        }
        Ok(())
    }

    pub fn create_layers(self, image_writer: &mut ImageWriter<'a>) -> anyhow::Result<()> {
        for bin in self.bins.into_iter() {
            image_writer.add_layer(self.name, bin.into_iter())?;
        }

        Ok(())
    }
}

impl Display for LayerPacker<'_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "LayerPacker:")?;
        writeln!(
            f,
            "- Target size: {:#.1}",
            Byte::from(self.target_size).get_appropriate_unit(UnitType::Decimal)
        )?;
        writeln!(f, "- Total bins: {}", self.bins.len())?;
        let total_effective_size: u64 = self.bins.iter().map(|v| v.effective_size).sum();
        writeln!(
            f,
            "- Total effective size: {:#.1}",
            Byte::from(total_effective_size).get_appropriate_unit(UnitType::Decimal)
        )?;
        let total_size: u64 = self.bins.iter().map(|v| v.total_size).sum();
        writeln!(
            f,
            "- Total size: {:#.1}",
            Byte::from(total_size).get_appropriate_unit(UnitType::Decimal)
        )?;
        writeln!(f, "- Bins:")?;
        for (idx, bin) in self.bins.iter().enumerate() {
            writeln!(f, "  - {idx:>3}: {}", bin)?;
        }
        Ok(())
    }
}
