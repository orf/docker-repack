use crate::image_parser::layer_reader::Layer;
use oci_spec::image::{ImageIndex, ImageManifest};
use std::fs::File;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LayerID(usize);

pub struct Image {
    layers: Vec<Layer>,
    manifest: ImageManifest,
}

impl Image {
    pub fn from_dir(image_dir: PathBuf) -> anyhow::Result<Self> {
        let blobs_dir = image_dir.join("blobs").join("sha256");
        let index_path = image_dir.join("index.json");
        let index_file = File::open(index_path)?;
        let index: ImageIndex = serde_json::from_reader(index_file)?;
        let manifests = index.manifests();
        if manifests.len() != 1 {
            panic!(
                "Expected exactly one manifest in the index file, found {}",
                manifests.len()
            );
        }

        let manifest = &manifests[0];
        let digest = manifest.digest();

        let hash = digest.split_once(':').unwrap().1;

        let manifest_file = File::open(blobs_dir.join(hash))?;
        let manifest_file: ImageManifest = serde_json::from_reader(manifest_file)?;

        let layers = manifest_file.layers();
        let layers: Vec<_> = layers
            .iter()
            .enumerate()
            .map(|(idx, descriptor)| {
                let digest = descriptor.digest().split_once(':').unwrap().1.to_string();
                let size = descriptor.size() as u64;
                Layer {
                    id: LayerID(idx),
                    path: blobs_dir.join(&digest),
                    digest,
                    size,
                    regular_file_count: 0,
                }
            })
            .collect();

        Ok(Self {
            layers,
            manifest: manifest_file,
        })
    }

    pub fn set_layer_file_count(&mut self, layer_index: LayerID, file_count: usize) {
        self.layers[layer_index.0].regular_file_count = file_count;
    }

    pub fn layers(&self) -> &Vec<Layer> {
        &self.layers
    }

    // pub fn layers_with_compression_filters<'a>(
    //     &'a self,
    //     layer_contents: &'a LayerContents,
    // ) -> impl Iterator<Item = (&'a Layer, PathFilter<'a>)> {
    //     self.layers.iter().filter_map(|layer| {
    //         let layer_filter = layer_contents.create_compression_filter(layer.id);
    //         layer_filter.map(|filter| (layer, filter))
    //     })
    // }
}
