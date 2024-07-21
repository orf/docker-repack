use crate::image_parser::item_operation::ItemOperation;
use crate::image_parser::TarItem;

pub struct LayerOperations {
    pub(crate) operations: Vec<(ItemOperation, TarItem)>,
}

impl LayerOperations {
    pub fn from_tar_items(paths: impl Iterator<Item = TarItem>) -> anyhow::Result<Self> {
        Ok(Self {
            operations: paths
                .into_iter()
                .map(|item| (ItemOperation::from_tar_item(&item), item))
                .collect(),
        })
    }

    pub fn regular_file_count(&self) -> usize {
        self.operations
            .iter()
            .filter(|(_, item)| item.is_regular_file)
            .count()
    }

    pub fn removed_files(&self) -> impl Iterator<Item = (&String, &TarItem)> {
        self.operations
            .iter()
            .filter_map(|(operation, item)| match operation {
                ItemOperation::RemoveFile(path) => Some((path, item)),
                _ => None,
            })
    }

    pub fn removed_prefixes(&self) -> impl Iterator<Item = (&String, &TarItem)> {
        self.operations
            .iter()
            .filter_map(|(operation, item)| match operation {
                ItemOperation::RemovePrefix(path) => Some((path, item)),
                _ => None,
            })
    }

    pub fn added(self) -> impl Iterator<Item = (String, TarItem)> {
        self.operations
            .into_iter()
            .filter_map(|(operation, item)| match operation {
                ItemOperation::Add(path) => Some((path, item)),
                _ => None,
            })
    }
}
