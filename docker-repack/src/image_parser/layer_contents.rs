use crate::image_parser::image_reader::SourceLayerID;
use crate::image_parser::layer_operation::LayerOperations;
use crate::image_parser::TarItem;
use byte_unit::Byte;
use globset::GlobSet;
use std::collections::BTreeMap;
use std::ops::Bound::{Excluded, Unbounded};
use trie_rs::iter::KeysExt;

pub type PathMap = BTreeMap<String, TarItem>;

#[derive(Debug, Default, Clone)]
pub struct LayerContents {
    pub(crate) present_paths: PathMap,
}

impl LayerContents {
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
            new_contents.present_paths.insert(path.clone(), item);
        }

        new_contents
    }

    fn remove_path(&mut self, path: &String, layer_index: SourceLayerID) {
        if self.present_paths.remove(path).is_none() {
            if !path.ends_with('/') {
                // Try removing a directory, if it exists
                if self.present_paths.remove(&format!("{path}/")).is_some() {
                    return;
                }
            }
            panic!("Tried to remove non-existent path '{path}' in layer {layer_index:?}");
        }
    }

    pub fn exclude_globs(&mut self, glob_set: GlobSet) -> (usize, Byte) {
        let initial_count = self.present_paths.len();
        let initial_size = self.present_paths.values().map(|p| p.size).sum::<u64>();
        self.present_paths.retain(|path, item| {
            if let Some(link_target) = item.link_target() {
                return !glob_set.is_match(link_target);
            }
            !glob_set.is_match(path)
        });
        let new_count = self.present_paths.len();
        let new_size = self.present_paths.values().map(|p| p.size).sum::<u64>();
        (
            initial_count - new_count,
            Byte::from(initial_size - new_size),
        )
    }

    pub fn len(&self) -> usize {
        self.present_paths.len()
    }

    pub fn into_inner(self) -> PathMap {
        self.present_paths
    }
}
