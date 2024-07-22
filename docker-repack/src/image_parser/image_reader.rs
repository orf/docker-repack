use crate::image_parser::layer_reader::Layer;
use oci_spec::image::{ImageConfiguration, ImageIndex, ImageManifest};
use std::fs::File;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SourceLayerID(pub usize);

pub struct ImageReader {
    layers: Vec<Layer>,
    pub config: ImageConfiguration,
}

pub fn read_blob<'a, T: for<'de> serde::Deserialize<'de>>(
    blobs_dir: &Path,
    digest: &str,
) -> anyhow::Result<T> {
    let hash = digest.split_once(':').unwrap().1;
    let manifest_file = File::open(blobs_dir.join(hash))?;
    Ok(serde_json::from_reader(manifest_file)?)
}

impl ImageReader {
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
        let manifest_file: ImageManifest = read_blob(&blobs_dir, digest)?;

        let config_descriptor = manifest_file.config();
        let config: ImageConfiguration = read_blob(&blobs_dir, config_descriptor.digest())?;

        let layers = manifest_file.layers();
        let layers: Vec<_> = layers
            .iter()
            .enumerate()
            .map(|(idx, descriptor)| {
                let digest = descriptor.digest().split_once(':').unwrap().1.to_string();
                let size = descriptor.size() as u64;
                Layer {
                    id: SourceLayerID(idx),
                    path: blobs_dir.join(digest),
                    // digest,
                    size,
                }
            })
            .collect();

        Ok(Self { layers, config })
    }

    pub fn layers(&self) -> &Vec<Layer> {
        &self.layers
    }
}
