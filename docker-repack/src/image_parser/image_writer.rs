use crate::image_parser::image_reader::SourceLayerID;
use crate::image_parser::layer_reader::progress_reader;
use crate::image_parser::layer_writer::{LayerWriter, WrittenLayer};
use crate::image_parser::{HashAndSize, HashedWriter, ImageReader};
use anyhow::bail;
use chrono::Utc;
use indicatif::MultiProgress;
use itertools::Itertools;
use oci_spec::image::{
    Descriptor, HistoryBuilder, ImageIndexBuilder, ImageManifestBuilder, MediaType,
};
use rayon::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use tar::Entry;

// const ZSTD_OUTPUT_LEVEL: i32 = 19;
const ZSTD_OUTPUT_LEVEL: i32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NewLayerID(usize);

pub struct ImageWriter<'a> {
    directory: PathBuf,
    blobs_dir: PathBuf,
    temp_dir: PathBuf,
    layers: Vec<LayerWriter>,
    paths: HashMap<(SourceLayerID, &'a str), NewLayerID>,
}

impl<'a> ImageWriter<'a> {
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
            paths: HashMap::new(),
        })
    }

    pub fn add_layer(
        &mut self,
        name: &'static str,
        paths: impl Iterator<Item = (SourceLayerID, &'a str)>,
    ) -> anyhow::Result<()> {
        let layer_id = NewLayerID(self.layers.len());
        let mut expected_entries: usize = 0;
        for (source_layer_id, path) in paths.into_iter() {
            if let Some(t) = self.paths.insert((source_layer_id, path), layer_id) {
                bail!(
                    "Path '{path}' in source {source_layer_id:?} is present in multiple output \
                    layers: {} and {}",
                    t.0,
                    layer_id.0
                );
            }
            expected_entries += 1;
        }

        let path = self.temp_dir.join(format!("{name}-{}.tar", layer_id.0));

        self.layers
            .push(LayerWriter::create_layer(path, expected_entries)?);

        Ok(())
    }

    pub fn add_entry(
        &self,
        layer_id: SourceLayerID,
        item: Entry<impl Read>,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let binding = item.path()?;
        let path = binding.to_str().unwrap();

        if let Some(new_layer_id) = self.paths.get(&(layer_id, path)) {
            let layer = &self.layers[new_layer_id.0];
            layer.write_entry(&item, data)?;
        }

        Ok(())
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

    pub fn finish(self, mut image: ImageReader) -> anyhow::Result<Vec<WrittenLayer>> {
        let finished_layers: Result<Vec<_>, _> = self
            .layers
            .into_iter()
            .map(|layer| layer.finish())
            .collect();

        let finished_layers = finished_layers?;

        let progress = MultiProgress::new();

        let compressed_layers: Result<Vec<_>, _> = finished_layers
            .par_iter()
            .map(|layer| {
                compress_layer(&progress, self.blobs_dir.clone(), layer, ZSTD_OUTPUT_LEVEL)
            })
            .collect();
        drop(progress);
        let compressed_layers = compressed_layers?;

        let root_fs = image.config.rootfs_mut();
        let diff_ids = root_fs.diff_ids_mut();
        diff_ids.clear();
        diff_ids.extend(
            finished_layers
                .iter()
                .map(|layer| layer.hash.prefixed_hash()),
        );

        let history = image.config.history_mut();
        history.clear();
        let new_history: Result<Vec<_>, _> = finished_layers
            .iter()
            .map(|layer| {
                HistoryBuilder::default()
                    .created(Utc::now().to_rfc3339())
                    .created_by(format!("layer: {} entries", layer.entries))
                    .build()
            })
            .collect();
        history.extend(new_history?);

        let hash = Self::write_blob(&self.blobs_dir, &image.config)?;

        let manifest = ImageManifestBuilder::default()
            .config(Descriptor::new(
                MediaType::ImageConfig,
                hash.size as i64,
                hash.prefixed_hash(),
            ))
            .schema_version(2u32)
            .layers(
                compressed_layers
                    .iter()
                    .map(|hash_and_size| {
                        Descriptor::new(
                            MediaType::ImageLayer,
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
            serde_json::to_string(&manifest)?,
        )?;

        Ok(finished_layers)
    }
}

fn compress_layer(
    progress: &MultiProgress,
    blob_dir: PathBuf,
    layer: &WrittenLayer,
    level: i32,
) -> anyhow::Result<HashAndSize> {
    let input_file = File::open(&layer.path)?;
    let input_size = input_file.metadata()?.len();
    let tar_path = &layer.path;
    let output_path = blob_dir.join(tar_path.file_name().unwrap());
    let output_file = BufWriter::new(File::create(&output_path)?);
    let hash_writer = HashedWriter::new(output_file);
    let mut encoder = zstd::stream::Encoder::new(hash_writer, level)?;
    encoder.set_pledged_src_size(Some(input_size))?;

    let buf_reader = BufReader::new(input_file);
    let mut progress_reader = progress_reader(progress, input_size, buf_reader);

    std::io::copy(&mut progress_reader, &mut encoder)?;

    let hash_writer = encoder.finish()?;
    let (content, hash_and_size) = hash_writer.into_inner();
    content.into_inner()?;

    let path_with_hash = blob_dir.join(hash_and_size.raw_hash());
    std::fs::rename(output_path, path_with_hash)?;
    std::fs::remove_file(&layer.path)?;
    Ok(hash_and_size)
}
