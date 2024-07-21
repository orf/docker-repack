use crate::image_parser::path_filter::PathFilter;
use crate::image_parser::{ImageWriter, TarItem};
use byte_unit::{Byte, UnitType};
// use pack_it_up::offline::first_fit_decreasing::first_fit_decreasing as fit;
use itertools::Itertools;
use pack_it_up::online::first_fit::first_fit as fit;
use std::fmt::{Display, Formatter};

#[derive(Debug, Default)]
pub struct Bin<'a> {
    items: Vec<(&'a TarItem, Option<u64>)>,
}

impl<'a> Bin<'a> {
    pub fn total_size(&self) -> u64 {
        self.items
            .iter()
            .map(|(i, s)| s.unwrap_or(i.raw_size))
            .sum()
    }

    pub fn total_raw_size(&self) -> u64 {
        self.items.iter().map(|(i, _)| i.raw_size).sum::<u64>()
    }

    pub fn count(&self) -> u64 {
        self.items.len() as u64
    }

    pub fn into_filter(self) -> PathFilter<'a> {
        PathFilter::from_iter(
            self.items
                .into_iter()
                .map(|(item, s)| (item.layer_id, item.path.to_str().unwrap())),
        )
    }
}

impl Display for Bin<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let bytes = Byte::from(self.total_size()).get_appropriate_unit(UnitType::Decimal);
        write!(f, "Bin: {} items, {:#.1}", self.count(), bytes)
    }
}

#[derive(Debug, Default)]
pub struct LayerPacker<'a> {
    compressible_files: Vec<Bin<'a>>,
    poorly_compressible_files: Vec<Bin<'a>>,
    tiny_items: Bin<'a>,
}

impl<'a> LayerPacker<'a> {
    pub fn create_layers(self, image_writer: &mut ImageWriter<'a>) -> anyhow::Result<()> {
        image_writer.add_layer("directories-and-tiny-files", self.tiny_items.into_filter())?;

        for bin in self.compressible_files {
            image_writer.add_layer("compressible-files", bin.into_filter())?;
        }

        for bin in self.poorly_compressible_files {
            image_writer.add_layer("poorly-compressible-files", bin.into_filter())?;
        }

        Ok(())
    }
    pub fn pack_tiny_items(&mut self, items: impl Iterator<Item = &'a TarItem>) {
        self.tiny_items = Bin {
            items: items
                .sorted_by(|a, b| a.path.cmp(&b.path))
                .map(|i| (i, None))
                .collect(),
        };
    }

    // pub fn pack_compressible_files(
    //     &mut self,
    //     target_layer_size: usize,
    //     items: &'a [CompressionResult],
    // ) {
    //     let mut compressible_files = vec![];
    //     let mut poorly_compressible_files = vec![];
    //
    //     for item in items {
    //         if item.compression_ratio > 100 {
    //             compressible_files.push(item);
    //         } else {
    //             poorly_compressible_files.push(item);
    //         }
    //     }
    //
    //     let compressible_bins = fit(target_layer_size, compressible_files);
    //     let poorly_compressible_bins = fit(target_layer_size, poorly_compressible_files);
    //
    //     self.compressible_files = compressible_bins
    //         .into_iter()
    //         .map(|bin| Bin {
    //             items: bin
    //                 .contents()
    //                 .iter()
    //                 .map(|p| (&p.tar_item, Some(p.compressed_size)))
    //                 .collect(),
    //         })
    //         .collect();
    //     self.compressible_files
    //         .sort_by(|a, b| b.total_size().cmp(&a.total_size()).reverse());
    //
    //     self.poorly_compressible_files = poorly_compressible_bins
    //         .into_iter()
    //         .map(|bin| Bin {
    //             items: bin
    //                 .contents()
    //                 .iter()
    //                 .map(|p| (&p.tar_item, Some(p.compressed_size)))
    //                 .collect(),
    //         })
    //         .collect();
    //     self.poorly_compressible_files
    //         .sort_by(|a, b| b.total_size().cmp(&a.total_size()).reverse());
    // }
}

impl Display for LayerPacker<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "LayerPacker:")?;
        writeln!(f, " - Tiny: {}", self.tiny_items)?;
        writeln!(f, " - Compressible:")?;
        for (idx, bin) in self.compressible_files.iter().enumerate() {
            writeln!(f, "   - {idx:>3} {}", bin)?;
        }
        writeln!(f, " - Poorly compressible:")?;
        for (idx, bin) in self.poorly_compressible_files.iter().enumerate() {
            writeln!(f, "   - {idx:>3} {}", bin)?;
        }
        let total_items: u64 = self.tiny_items.count()
            + self
                .poorly_compressible_files
                .iter()
                .map(|v| v.count())
                .sum::<u64>()
            + self
                .compressible_files
                .iter()
                .map(|v| v.count())
                .sum::<u64>();
        writeln!(f, " - Total: {} items", total_items)?;
        Ok(())
    }
}
