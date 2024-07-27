use crate::image_parser::image_reader::SourceLayerID;
use crate::image_parser::layer_reader::progress_reader;
use crate::image_parser::layer_writer::{LayerType, LayerWriter, WrittenLayer};
use crate::image_parser::{utils, HashAndSize, HashedWriter, ImageReader};
use anyhow::bail;
use chrono::Utc;
use indicatif::{MultiProgress, ProgressDrawTarget};
use itertools::Itertools;
use oci_spec::image::{
    Descriptor, HistoryBuilder, ImageIndexBuilder, ImageManifestBuilder, MediaType,
};
use rayon::prelude::*;
use serde_json::json;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use byte_unit::{Byte, UnitType};
use tar::Entry;

const ZSTD_OUTPUT_LEVEL: i32 = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NewLayerID(usize);

pub struct ImageWriter<'a> {
    directory: PathBuf,
    blobs_dir: PathBuf,
    temp_dir: PathBuf,
    layers: Vec<LayerWriter>,
    paths: HashMap<(SourceLayerID, &'a str, Range<u64>), (NewLayerID, Option<PathBuf>)>,
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

    pub fn create_new_layer(
        &mut self,
        name: &'static str,
        type_: LayerType,
    ) -> anyhow::Result<(NewLayerID, &LayerWriter)> {
        let layer_id = NewLayerID(self.layers.len());
        let path = self.temp_dir.join(format!("{name}-{}.tar", layer_id.0));
        let layer = LayerWriter::create_layer(path, type_)?;
        self.layers.push(layer);
        Ok((layer_id, &self.layers[layer_id.0]))
    }

    pub fn add_layer_paths(
        &mut self,
        name: &'static str,
        paths: impl Iterator<Item=(SourceLayerID, &'a str, Range<u64>, Option<PathBuf>)>,
        type_: LayerType,
    ) -> anyhow::Result<()> {
        let (layer_id, _) = self.create_new_layer(name, type_)?;
        for (source_layer_id, path, byte_range, override_path) in paths.into_iter() {
            if let Some((t, old_override_path)) = self.paths.insert(
                (source_layer_id, path, byte_range),
                (layer_id, override_path),
            ) {
                bail!(
                    "Path '{path}' in source {source_layer_id:?} is present in multiple output \
                    layers: {} and {} (override path: {old_override_path:?})",
                    t.0,
                    layer_id.0
                );
            }
        }

        Ok(())
    }

    pub fn add_entry(
        &self,
        layer_id: SourceLayerID,
        item: &mut Entry<impl Read>,
        chunk_size: Option<u64>,
    ) -> anyhow::Result<()> {
        let item_path = item.path()?.into_owned();
        let path = item_path.to_str().unwrap();

        if let Some(chunk_size) = chunk_size {
            if item.header().entry_type() == tar::EntryType::Regular && item.size() > chunk_size {
                for (idx, chunk_range) in
                    utils::byte_range_chunks(item.size(), chunk_size).enumerate()
                {
                    match (idx, self.paths.get(&(layer_id, path, chunk_range.clone()))) {
                        (0, None) => return Ok(()),
                        (_, Some((new_layer_id, Some(override_path)))) => {
                            let mut cloned_header = item.header().clone();
                            let layer = &self.layers[new_layer_id.0];
                            let size = chunk_range.end - chunk_range.start;
                            cloned_header.set_size(size);
                            layer.write_file(
                                cloned_header,
                                override_path,
                                item.take(size),
                                chunk_range,
                            )?;
                        }
                        (idx, Some((_, None))) => {
                            bail!(
                                "Missing override path for chunk {} of file {} (range {:?})",
                                idx,
                                item_path.display(),
                                chunk_size
                            );
                        }
                        (idx, None) => {
                            bail!(
                                "Missing a layer for chunk {} of file {} (range {:?})",
                                idx,
                                item_path.display(),
                                chunk_size
                            );
                        }
                    }
                }
            }
        }

        let cloned_header = item.header().clone();

        let byte_range = 0..item.size();

        if let Some((new_layer_id, _)) = self.paths.get(&(layer_id, path, byte_range.clone())) {
            let layer = &self.layers[new_layer_id.0];
            match item.header().entry_type() {
                tar::EntryType::Directory => {
                    layer.write_directory(cloned_header, &item_path)?;
                }
                tar::EntryType::Link | tar::EntryType::Symlink => {
                    let target_path = item.link_name()?.expect("Link entry without link name");
                    layer.write_link(cloned_header, &item_path, &target_path)?;
                }
                tar::EntryType::Regular if item.size() == 0 => {
                    layer.write_empty_file(cloned_header, &item_path)?;
                }
                tar::EntryType::Regular if item.size() > 0 => {
                    layer.write_file(
                        cloned_header,
                        &item_path,
                        item.take(byte_range.end),
                        byte_range,
                    )?;
                }
                _ => {
                    todo!();
                }
            }
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

    pub fn finish_writing_layers(&mut self) -> anyhow::Result<Vec<WrittenLayer>> {
        let finished_layers: Result<Vec<_>, _> = self
            .layers
            .drain(0..)
            .into_iter()
            .map(|layer| layer.finish())
            .collect();

        Ok(finished_layers?)
    }

    pub fn compress_layers(
        &self,
        progress: &MultiProgress,
        finished_layers: Vec<WrittenLayer>,
    ) -> anyhow::Result<Vec<(WrittenLayer, HashAndSize)>> {
        let compressed_layers: Result<Vec<_>, _> = finished_layers
            .into_par_iter()
            .map(|layer| {
                compress_layer(&progress, self.blobs_dir.clone(), &layer, ZSTD_OUTPUT_LEVEL)
                    .map(|v| (layer, v))
            })
            .collect();
        Ok(compressed_layers?)
    }

    pub fn write_index(
        self,
        finished_layers: &Vec<(WrittenLayer, HashAndSize)>,
        mut image: ImageReader,
    ) -> anyhow::Result<()> {
        let root_fs = image.config.rootfs_mut();
        let diff_ids = root_fs.diff_ids_mut();
        diff_ids.clear();
        diff_ids.extend(
            finished_layers
                .iter()
                .map(|(layer, _)| layer.hash.prefixed_hash()),
        );

        let history = image.config.history_mut();
        history.clear();
        let new_history: Result<Vec<_>, _> = finished_layers
            .iter()
            .map(|(layer, _)| {
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
                finished_layers
                    .iter()
                    .map(|(_, hash_and_size)| {
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
        Ok(())
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
    // std::fs::remove_file(&layer.path)?;
    Ok(hash_and_size)
}
