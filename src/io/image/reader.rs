use crate::io::compression::CompressionType;
use crate::io::image::writer::ImageWriter;
use crate::io::layer::reader::{CompressedLayer, DecompressedLayer};
use anyhow::anyhow;
use indicatif::MultiProgress;
use oci_spec::image::{ImageConfiguration, ImageIndex, ImageManifest};
use rayon::prelude::*;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct SourceLayerID(pub usize);

impl Display for SourceLayerID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Layer {:<2}", self.0)
    }
}

pub struct ImageReader {
    layers: Vec<CompressedLayer>,
    pub config: ImageConfiguration,
}

pub fn read_blob<'a, T: for<'de> serde::Deserialize<'de>>(blobs_dir: &Path, digest: &str) -> anyhow::Result<T> {
    let hash = digest.split_once(':').unwrap().1;
    let manifest_file = File::open(blobs_dir.join(hash))?;
    Ok(serde_json::from_reader(manifest_file)?)
}

impl ImageReader {
    pub fn from_dir(image_dir: &Path) -> anyhow::Result<ImageReader> {
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

        let manifest_layers = manifest_file.layers();
        let layers: Result<Vec<_>, anyhow::Error> = manifest_layers
            .iter()
            .enumerate()
            .map(|(idx, descriptor)| {
                let digest = descriptor.digest().split_once(':').unwrap().1.to_string();
                let size = descriptor.size() as u64;
                let media_type = descriptor.media_type();
                let compression = CompressionType::from_media_type(media_type)?;
                let path = blobs_dir.join(digest);
                let compressed_size = path.metadata()?.len();
                Ok(CompressedLayer {
                    id: SourceLayerID(idx),
                    path,
                    compression,
                    size,
                    compressed_size,
                })
            })
            .collect();

        let layers = layers?;

        Ok(Self { layers, config })
    }

    pub fn decompress_layers(
        self,
        image_writer: &ImageWriter,
        progress: &MultiProgress,
    ) -> anyhow::Result<(Vec<DecompressedLayer>, ImageConfiguration)> {
        let decompressed_layers: Result<Vec<_>, anyhow::Error> = self
            .layers
            .into_par_iter()
            .map(|layer| {
                let layer_file_name = layer.path.file_name().ok_or(anyhow!("No path name"))?;
                let new_path = image_writer.temp_dir.join(layer_file_name).with_extension("raw");
                layer.decompress(progress, new_path)
            })
            .collect();
        let decompressed_layers = decompressed_layers?;
        Ok((decompressed_layers, self.config))
    }

    pub fn layers(&self) -> &Vec<CompressedLayer> {
        &self.layers
    }

    pub fn compressed_size(&self) -> u64 {
        self.layers.iter().map(|l| l.compressed_size).sum()
    }
}
