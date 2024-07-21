use crate::image_parser::layer_writer::{LayerWriter, WrittenLayer};
use crate::image_parser::path_filter::PathFilter;
use crate::image_parser::TarItem;
use anyhow::bail;
use std::io::Read;
use std::path::PathBuf;
use tar::Entry;

pub struct ImageWriter<'a> {
    directory: PathBuf,
    layers: Vec<LayerWriter<'a>>,
}

impl<'a> ImageWriter<'a> {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            layers: vec![],
        }
    }

    pub fn add_layer(&mut self, name: &'static str, filter: PathFilter<'a>) -> anyhow::Result<()> {
        let path = self
            .directory
            .join(format!("layer-{}.tar.zst", self.layers.len()));
        self.layers
            .push(LayerWriter::create_layer(name, path, filter)?);
        Ok(())
    }

    pub fn add_item(
        &self,
        tar_item: TarItem,
        item: Entry<impl Read>,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let path = tar_item.path.to_str().unwrap();

        for (idx, writer) in self
            .layers
            .iter()
            .filter(|layer| layer.contains_path(tar_item.layer_id, path))
            .enumerate()
        {
            if idx != 0 {
                bail!("Path '{path}' is present in multiple layers");
            }

            writer.write_entry(&item, data)?
        }

        Ok(())
    }

    pub fn finish(self) -> anyhow::Result<Vec<WrittenLayer>> {
        let results: Result<Vec<_>, _> = self
            .layers
            .into_iter()
            .map(|layer| layer.finish())
            .collect();
        results
    }
}
