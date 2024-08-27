use crate::content::operations::LayerOperations;
use crate::io::image::reader::SourceLayerID;
use crate::tar_item::TarItem;
use globset::GlobSet;
use itertools::Itertools;
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};
use trie_rs::iter::KeysExt;

pub type PathMap = BTreeMap<String, TarItem>;

#[derive(Default, Debug, Clone)]
pub struct Count {
    pub count: u64,
    pub size: u64,
}

impl Count {
    pub fn increment(&mut self, count: u64, size: u64) {
        self.count += count;
        self.size += size;
    }

    pub fn increment_tar_item(&mut self, item: &TarItem) {
        self.increment(1, item.size)
    }
}

#[derive(Default, Debug, Clone)]
pub struct MergedLayerContent {
    pub added_files: Count,
    pub removed_files: Count,
    pub excluded_files: Count,
    pub(crate) present_paths: PathMap,
}

impl MergedLayerContent {
    pub fn merge_operations(&mut self, layer_operations: LayerOperations) -> Self {
        let mut new_contents = self.clone();

        for (path, item) in layer_operations.removed_files() {
            new_contents.remove_path(path, item.layer_id);
        }

        for (path, item) in layer_operations.removed_prefixes() {
            // to-do: this kinda sucks
            let sub_paths = self
                .present_paths
                .range::<String, _>((Excluded(path), Unbounded))
                .keys()
                .take_while(|p| p.starts_with(path));
            for path in sub_paths {
                new_contents.remove_path(path, item.layer_id);
            }
        }

        for (path, item) in layer_operations.added() {
            new_contents.add_path(path, item);
        }

        new_contents
    }

    fn add_path(&mut self, path: String, item: TarItem) {
        self.added_files.increment_tar_item(&item);
        if let Some(item) = self.present_paths.insert(path, item) {
            self.removed_files.increment_tar_item(&item);
        }
    }

    fn remove_path(&mut self, path: &String, layer_index: SourceLayerID) {
        match self.present_paths.remove(path) {
            Some(item) => self.removed_files.increment_tar_item(&item),
            None => {
                if !path.ends_with('/') {
                    // Try removing a directory, if it exists
                    if let Some(item) = self.present_paths.remove(&format!("{path}/")) {
                        self.removed_files.increment_tar_item(&item);
                        return;
                    }
                }
                panic!("Tried to remove non-existent path '{path}' in layer {:?}", layer_index);
            }
        }
    }

    pub fn exclude_globs(&mut self, glob_set: GlobSet) {
        let initial_count = self.present_paths.len() as u64;
        let initial_size = self.present_paths.values().map(|p| p.size).sum::<u64>();
        self.present_paths.retain(|path, item| {
            if let Some(link_target) = item.link_target() {
                return !glob_set.is_match(link_target);
            }
            !glob_set.is_match(path)
        });
        let new_count = self.present_paths.len() as u64;
        let new_size = self.present_paths.values().map(|p| p.size).sum::<u64>();
        self.excluded_files
            .increment(initial_count - new_count, initial_size - new_size);
    }

    pub fn len(&self) -> usize {
        self.present_paths.len()
    }

    pub fn non_empty_files(&self) -> impl Iterator<Item = &TarItem> {
        self.present_paths.values().filter(|p| p.is_file() && p.size > 0)
    }

    pub fn unique_non_empty_files_count(&self) -> usize {
        self.non_empty_files().filter_map(|p| p.content_hash()).unique().count()
    }

    pub fn total_size(&self) -> u64 {
        self.present_paths.values().map(|v| v.size).sum()
    }

    pub fn into_inner(self) -> PathMap {
        self.present_paths
    }
}
