use crate::index::ImageItem;
use anyhow::bail;
use itertools::Itertools;
use std::cmp::PartialEq;
use std::collections::HashMap;
use std::path::PathBuf;
use tar::{Builder, EntryType};

use crate::progress::{display_bytes, progress_iter};
#[cfg(test)]
use std::collections::HashSet;
use std::fmt::{Debug, Display, Formatter};
use std::io::Write;
use tracing::instrument;

#[derive(Debug, Eq, PartialEq, Copy, Clone, Ord, PartialOrd, strum::Display)]
pub enum LayerType {
    Small,
    Standard,
    Supersized,
}

#[derive(Debug)]
pub struct OutputLayer<'a> {
    pub type_: LayerType,
    items: Vec<&'a ImageItem<'a>>,
}

impl Display for OutputLayer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} items={} size={:#.1} compressed={:#.1}",
            self.type_,
            self.items.len(),
            display_bytes(self.raw_size()),
            display_bytes(self.compressed_size()),
        )
    }
}

impl<'a> OutputLayer<'a> {
    pub fn from_items(
        type_: LayerType,
        items: &[&'a ImageItem<'a>],
        hardlink_map: &HashMap<PathBuf, Vec<&'a ImageItem>>,
        duplicate_map: &HashMap<[u8; 32], Vec<&&'a ImageItem>>,
    ) -> Self {
        let mut layer = OutputLayer { type_, items: vec![] };
        for item in items {
            layer.add_item(item, hardlink_map, duplicate_map);
        }
        layer
    }

    #[inline]
    pub fn add_item(
        &mut self,
        item: &'a ImageItem<'a>,
        hardlink_map: &HashMap<PathBuf, Vec<&'a ImageItem>>,
        duplicate_map: &HashMap<[u8; 32], Vec<&&'a ImageItem>>,
    ) {
        self.items.push(item);
        if let Some(items) = hardlink_map.get(&item.path) {
            self.items.extend(items);
        }
        if let Some(duplicates) = duplicate_map.get(&item.hash) {
            self.items
                .extend(duplicates.iter().filter(|dup| dup.path != item.path).map(|&&item| item));
        }
    }

    pub fn compressed_size(&self) -> u64 {
        self.items.iter().map(|item| item.compressed_size).sum()
    }

    pub fn raw_size(&self) -> u64 {
        self.items.iter().map(|item| item.raw_size).sum()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    #[inline(always)]
    fn to_writer_from_iterable<T: Write>(
        &self,
        out: &'a mut T,
        items: impl Iterator<Item = &'a &'a ImageItem<'a>>,
    ) -> anyhow::Result<&'a mut T> {
        let mut archive = Builder::new(out);
        for item in items {
            if item.content.is_empty() {
                archive.append(&item.header, std::io::empty())?;
            } else {
                archive.append(&item.header, item.content)?;
            }
        }
        Ok(archive.into_inner()?)
    }

    #[inline(always)]
    pub fn to_writer_with_progress<T: Write>(
        &'a self,
        name: &'static str,
        out: &'a mut T,
    ) -> anyhow::Result<&'a mut T> {
        self.to_writer_from_iterable(out, progress_iter(name, self.items.iter()))
    }

    #[inline(always)]
    #[allow(dead_code)]
    pub fn to_writer<T: Write>(&'a self, out: &'a mut T) -> anyhow::Result<&'a mut T> {
        self.to_writer_from_iterable(out, self.items.iter())
    }

    #[cfg(test)]
    pub fn paths(&self) -> Vec<&std::path::Path> {
        self.items.iter().map(|item| item.path.as_path()).collect_vec()
    }
}

pub struct OutputLayers<'a> {
    layers: Vec<OutputLayer<'a>>,
}

impl Display for OutputLayers<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let compressed_size = self
            .all_layers()
            .iter()
            .map(|layer| layer.compressed_size())
            .sum::<u64>();
        let raw_size = self.all_layers().iter().map(|layer| layer.raw_size()).sum::<u64>();
        f.write_fmt(format_args!(
            "size={} raw={:#.1} compressed={:#.1}",
            self.len(),
            display_bytes(raw_size),
            display_bytes(compressed_size)
        ))
    }
}

impl<'a> OutputLayers<'a> {
    #[instrument(name = "packing files", skip_all)]
    pub fn pack_items(
        items_map: &'a HashMap<PathBuf, ImageItem>,
        small_items_threshold: u64,
        target_size: u64,
    ) -> anyhow::Result<OutputLayers<'a>> {
        let (hardlink_items, mut items): (Vec<_>, Vec<_>) = items_map
            .values()
            .partition(|item| item.header.entry_type() == EntryType::Link);

        let mut hardlink_map: HashMap<PathBuf, Vec<&ImageItem>> = HashMap::new();
        for item in hardlink_items {
            if let Some(link_name) = item.header.link_name()? {
                hardlink_map.entry(link_name.to_path_buf()).or_default().push(item);
            } else {
                bail!("Link item without link name: {}", item.path.display());
            }
        }

        items.sort_by(|e1, e2| e1.path.cmp(&e2.path));

        let (small_items, standard_items): (Vec<_>, Vec<_>) = items.into_iter().partition(|item| {
            (item.raw_size <= small_items_threshold || item.compressed_size <= small_items_threshold)
                && matches!(
                    item.header.entry_type(),
                    EntryType::Regular | EntryType::Symlink | EntryType::Directory
                )
        });

        let (standard_items, extra_large_items): (Vec<_>, Vec<_>) = standard_items
            .into_iter()
            .partition(|item| item.compressed_size <= target_size);

        let files_by_hash = standard_items.iter().into_group_map_by(|v| v.hash);
        let small_layer = OutputLayer::from_items(LayerType::Small, &small_items, &hardlink_map, &files_by_hash);

        let unique_files_by_hash = standard_items.iter().unique_by(|v| v.hash).copied().collect_vec();

        let mut layers: Vec<OutputLayer> = Vec::with_capacity(14);
        'outer: for item in unique_files_by_hash {
            for layer in layers.iter_mut() {
                if layer.compressed_size() + item.compressed_size <= target_size {
                    layer.add_item(item, &hardlink_map, &files_by_hash);
                    continue 'outer;
                }
            }
            layers.push(OutputLayer::from_items(
                LayerType::Standard,
                &[item],
                &hardlink_map,
                &files_by_hash,
            ))
        }
        layers.push(small_layer);
        for item in extra_large_items {
            layers.push(OutputLayer::from_items(
                LayerType::Supersized,
                &[item],
                &hardlink_map,
                &files_by_hash,
            ))
        }

        Ok(OutputLayers { layers })
    }

    pub fn all_layers(&self) -> &[OutputLayer<'a>] {
        self.layers.as_slice()
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }

    #[cfg(test)]
    pub fn layers_by_type(&self, type_: LayerType) -> impl Iterator<Item = &OutputLayer<'a>> {
        self.all_layers().iter().filter(move |layer| layer.type_ == type_)
    }

    #[cfg(test)]
    pub fn small_layers(&self) -> Vec<&OutputLayer<'a>> {
        self.layers_by_type(LayerType::Small).collect_vec()
    }

    #[cfg(test)]
    pub fn supersized_layers(&self) -> Vec<&OutputLayer<'a>> {
        self.layers_by_type(LayerType::Supersized).collect_vec()
    }

    #[cfg(test)]
    fn layer_set(&self) -> HashSet<&std::path::Path> {
        self.layers.iter().flat_map(|layer| layer.paths()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::ImageItems;

    use crate::test_utils::{add_dir, add_file, add_hardlink, compare_paths, setup_tar};

    #[test]
    fn test_pack_items_works() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/small.txt", b"small");
        add_file(&mut tar_1, "test/large.txt", b"larger content value");
        let data = tar_1.into_inner().unwrap();

        let items = ImageItems::from_data(data, 3);
        let content = items.get_image_content().unwrap();

        let items = ImageItem::items_from_data(content, 1).unwrap();

        let packed = OutputLayers::pack_items(&items, 100, 10).unwrap();
        compare_paths(
            packed.small_layers()[0].paths(),
            vec!["test/", "test/small.txt", "test/large.txt"],
        );

        let packed = OutputLayers::pack_items(&items, 1, 10).unwrap();
        compare_paths(packed.small_layers()[0].paths(), vec!["test/"]);
    }

    #[test]
    fn test_pack_items_simple_hardlinks() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/small.txt", b"small");
        add_hardlink(&mut tar_1, "test/small-link.txt", "test/small.txt");
        let data = tar_1.into_inner().unwrap();
        let items = ImageItems::from_data(data, 3);
        let content = items.get_image_content().unwrap();
        let items = ImageItem::items_from_data(content, 1).unwrap();

        let packed = OutputLayers::pack_items(&items, 5, 10).unwrap();
        compare_paths(
            packed.layer_set().iter().collect_vec(),
            vec!["test/", "test/small.txt", "test/small-link.txt"],
        );
        compare_paths(
            packed.small_layers()[0].paths(),
            vec!["test/", "test/small.txt", "test/small-link.txt"],
        );

        let packed = OutputLayers::pack_items(&items, 2, 10).unwrap();
        compare_paths(packed.small_layers()[0].paths(), vec!["test/"]);
    }

    #[test]
    fn test_pack_duplicate_items() {
        let mut tar_1 = setup_tar();
        add_file(&mut tar_1, "one.txt", b"content1");
        add_file(&mut tar_1, "two.txt", b"content1");
        add_file(&mut tar_1, "three.txt", b"content2");
        let data = tar_1.into_inner().unwrap();

        let items = ImageItems::from_data(data, 3);
        let content = items.get_image_content().unwrap();

        let items = ImageItem::items_from_data(content, 1).unwrap();

        let target_size = items[&PathBuf::from("one.txt")].compressed_size;

        let packed = OutputLayers::pack_items(&items, 1, target_size).unwrap();
        compare_paths(
            packed.layer_set().iter().collect_vec(),
            vec!["two.txt", "one.txt", "three.txt"],
        );
        compare_paths(packed.small_layers()[0].paths(), vec![]);
        compare_paths(packed.layers[0].paths(), vec!["one.txt", "two.txt"]);
        compare_paths(packed.layers[1].paths(), vec!["three.txt"]);
    }

    #[test]
    fn test_pack_large_items() {
        let mut tar_1 = setup_tar();
        add_file(&mut tar_1, "one.txt", b"content1");
        add_file(&mut tar_1, "two.txt", b"content1234567890");
        let data = tar_1.into_inner().unwrap();

        let items = ImageItems::from_data(data, 2);
        let content = items.get_image_content().unwrap();
        let items = ImageItem::items_from_data(content, 1).unwrap();

        let target_size = items[&PathBuf::from("one.txt")].compressed_size;

        let packed = OutputLayers::pack_items(&items, 1, target_size).unwrap();
        compare_paths(packed.layer_set().iter().collect_vec(), vec!["two.txt", "one.txt"]);
        compare_paths(packed.small_layers()[0].paths(), vec![]);
        compare_paths(packed.layers[0].paths(), vec!["one.txt"]);
        compare_paths(packed.supersized_layers()[0].paths(), vec!["two.txt"]);
    }
}
