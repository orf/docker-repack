use crate::io::compression::write::compress_file;
use crate::io::hashed_writer::{HashAndSize, HashedWriter};
use crate::io::layer::writer::{LayerWriter, WrittenLayer};
use chrono::Utc;
use indicatif::MultiProgress;
use itertools::Itertools;
use oci_spec::image::{
    Descriptor, HistoryBuilder, ImageConfiguration, ImageIndexBuilder, ImageManifestBuilder,
    MediaType,
};
use rayon::prelude::*;
use serde_json::json;
use std::fmt::{Display, Formatter};
use std::path::{Path, PathBuf};
use tracing::info;
use zstd::zstd_safe::CompressionLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct NewLayerID(usize);

impl Display for NewLayerID {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "New layer {:<2}", self.0)
    }
}

pub struct ImageWriter {
    pub directory: PathBuf,
    pub blobs_dir: PathBuf,
    pub temp_dir: PathBuf,
    pub layers: Vec<LayerWriter>,
}

impl ImageWriter {
    pub fn new(directory: PathBuf) -> anyhow::Result<Self> {
        let blobs_dir = directory.join("blobs").join("sha256");
        let temp_dir = directory.join("temp");
        std::fs::create_dir_all(&blobs_dir)?;
        std::fs::create_dir_all(&temp_dir)?;
        std::fs::write(
            directory.join("oci-layout"),
            json!({"imageLayoutVersion": "1.0.0"}).to_string(),
        )?;
        Ok(Self {
            directory,
            blobs_dir,
            temp_dir,
            layers: Vec::new(),
        })
    }

    pub fn remove_temp_files(&self) -> std::io::Result<()> {
        std::fs::remove_dir_all(&self.temp_dir)
    }

    pub fn get_layer(&mut self, new_layer_id: NewLayerID) -> &mut LayerWriter {
        &mut self.layers[new_layer_id.0]
    }

    pub fn create_new_layer(&mut self, name: &'static str) -> anyhow::Result<NewLayerID> {
        let layer_id = NewLayerID(self.layers.len());
        let path = self.temp_dir.join(format!("{name}-{}.tar", layer_id.0));
        let layer = LayerWriter::create_layer(layer_id, path)?;
        self.layers.push(layer);
        Ok(layer_id)
    }

    pub fn write_blob<T: serde::Serialize>(
        blobs_dir: &Path,
        item: T,
    ) -> anyhow::Result<HashAndSize> {
        let mut writer = HashedWriter::new(vec![]);
        serde_json::to_writer_pretty(&mut writer, &item)?;
        let (content, hash_and_size) = writer.into_inner();
        std::fs::write(blobs_dir.join(hash_and_size.raw_hash()), content)?;
        Ok(hash_and_size)
    }

    pub fn finish_writing_layers(&mut self) -> anyhow::Result<Vec<WrittenLayer>> {
        info!("Finishing writing layers");
        let finished_layers: Result<Vec<_>, _> =
            self.layers.drain(0..).map(|layer| layer.finish()).collect();
        info!("Layers written");
        finished_layers
    }

    pub fn write_uncompressed_layers(
        &self,
        finished_layers: Vec<WrittenLayer>,
    ) -> anyhow::Result<Vec<(WrittenLayer, HashAndSize)>> {
        info!("Creating uncompressed layers");
        let uncompressed_layers: Result<Vec<_>, anyhow::Error> = finished_layers
            .into_par_iter()
            .map(|layer| {
                let path_with_hash = self.blobs_dir.join(layer.hash.raw_hash());
                std::fs::rename(&layer.path, path_with_hash)?;
                let cloned_hash = layer.hash.clone();
                Ok((layer, cloned_hash))
            })
            .collect();
        let uncompressed_layers = uncompressed_layers?;
        info!("{} uncompressed layers created", uncompressed_layers.len());
        Ok(uncompressed_layers)
    }

    pub fn write_compressed_layers(
        &self,
        progress: &MultiProgress,
        finished_layers: Vec<WrittenLayer>,
        compression_level: CompressionLevel,
    ) -> anyhow::Result<Vec<(WrittenLayer, HashAndSize)>> {
        info!(
            "Compressing {} layers with level {}",
            finished_layers.len(),
            compression_level
        );
        let compressed_layers: Result<Vec<_>, _> = finished_layers
            .into_par_iter()
            .map(|layer| {
                let output_path = &self.blobs_dir.join(layer.path.file_name().unwrap());
                let hash_and_size = compress_file(
                    progress,
                    output_path,
                    &layer.path,
                    compression_level,
                    format!("Compressing {}", layer.id),
                )?;
                let path_with_hash = self.blobs_dir.join(hash_and_size.raw_hash());
                std::fs::rename(output_path, path_with_hash)?;
                Ok((layer, hash_and_size))
            })
            .collect();
        info!("Layers compressed");
        compressed_layers
    }

    pub fn write_index(
        self,
        finished_layers: &[(WrittenLayer, HashAndSize)],
        mut config: ImageConfiguration,
        skip_compression: bool,
        #[cfg(feature = "split_files")] entrypoint_override: Option<Vec<String>>,
    ) -> anyhow::Result<()> {
        let root_fs = config.rootfs_mut();
        let diff_ids = root_fs.diff_ids_mut();
        diff_ids.clear();
        diff_ids.extend(
            finished_layers
                .iter()
                .map(|(layer, _)| layer.hash.prefixed_hash()),
        );
        #[cfg(feature = "split_files")]
        if let Some(mut entrypoint_override) = entrypoint_override {
            if let Some(ref img_config) = config.config() {
                let mut cloned = img_config.clone();
                let new_entrypoint = match cloned.entrypoint() {
                    None => entrypoint_override,
                    Some(old_entrypoint) => {
                        entrypoint_override.extend(old_entrypoint.iter().cloned());
                        entrypoint_override
                    }
                };
                info!("Overriding entrypoint:");
                info!("old: {:?}", cloned.entrypoint());
                info!("new: {:?}", new_entrypoint);
                cloned.set_entrypoint(Some(new_entrypoint));
                config.set_config(Some(cloned));
            }
        }

        let history = config.history_mut();
        history.clear();
        let new_history: Result<Vec<_>, _> = finished_layers
            .iter()
            .map(|(layer, _)| {
                HistoryBuilder::default()
                    .created(Utc::now().to_rfc3339())
                    .created_by(format!("{layer}"))
                    .build()
            })
            .collect();
        history.extend(new_history?);

        let hash = Self::write_blob(&self.blobs_dir, &config)?;

        let layer_media_type = if skip_compression {
            MediaType::ImageLayer
        } else {
            MediaType::ImageLayerZstd
        };

        let manifest = ImageManifestBuilder::default()
            .config(Descriptor::new(
                MediaType::ImageConfig,
                hash.size as i64,
                hash.prefixed_hash(),
            ))
            .schema_version(2u32)
            .layers(
                finished_layers
                    .iter()
                    .map(|(_, hash_and_size)| {
                        Descriptor::new(
                            layer_media_type.clone(),
                            hash_and_size.size as i64,
                            hash_and_size.prefixed_hash(),
                        )
                    })
                    .collect_vec(),
            )
            .build()?;

        let manifest_hash = Self::write_blob(&self.blobs_dir, manifest)?;

        let manifest = ImageIndexBuilder::default()
            .schema_version(2u32)
            .manifests(vec![Descriptor::new(
                MediaType::ImageManifest,
                manifest_hash.size as i64,
                manifest_hash.prefixed_hash(),
            )])
            .build()?;

        std::fs::write(
            self.directory.join("index.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        Ok(())
    }
}
