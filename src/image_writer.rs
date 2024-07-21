use byte_unit::Byte;
use chrono::prelude::*;
use const_hex::Buffer;
use oci_spec::image::{
    Descriptor, ImageConfiguration, ImageIndex, ImageManifest, ImageManifestBuilder, MediaType,
};
use oci_spec::image::{HistoryBuilder, ImageIndexBuilder, RootFsBuilder};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

pub fn write_json(path: &Path, value: impl Serialize) -> anyhow::Result<()> {
    let file = File::create(path)?;
    serde_json::to_writer(&file, &value)?;
    Ok(())
}

pub fn write_blob(directory: &Path, value: impl Serialize) -> anyhow::Result<WrittenBlob> {
    let value = serde_json::to_string(&value)?;
    let mut hasher = Sha256::new();
    hasher.update(value.as_bytes());
    let sha_hash = hasher.finalize();

    let mut hash = Buffer::<32>::new();
    hash.format(&sha_hash.into());

    let path = directory.join(hash.as_str());
    let mut file = File::create(&path)?;
    let content = value.as_bytes();
    file.write_all(content)?;

    Ok(WrittenBlob::new(
        path,
        hash.clone(),
        content.len() as u64,
        hash.clone(),
        content.len() as u64,
    ))
}

pub struct WrittenBlob {
    pub path: PathBuf,
    pub compressed: HashAndSize,
    pub raw: HashAndSize,
}

impl WrittenBlob {
    pub fn new(
        path: PathBuf,
        compressed_hash: Buffer<32>,
        compressed_size: u64,
        raw_hash: Buffer<32>,
        raw_size: u64,
    ) -> Self {
        Self {
            path,
            compressed: HashAndSize {
                hash: compressed_hash,
                size: compressed_size,
            },
            raw: HashAndSize {
                hash: raw_hash,
                size: raw_size,
            },
        }
    }
}

pub struct ImageWriter {
    layers: Vec<WrittenBlob>,
}

impl ImageWriter {
    pub fn new(layers: Vec<WrittenBlob>) -> Self {
        Self { layers }
    }

    pub fn create_image_config(&self, from_path: &Path) -> anyhow::Result<ImageConfiguration> {
        let image_config = File::open(from_path)?;
        let mut image_config: ImageConfiguration = serde_json::from_reader(image_config)?;
        let diff_ids: Vec<_> = self
            .layers
            .iter()
            .map(|layer| layer.raw.prefixed_hash())
            .collect();

        let history: Vec<_> = self
            .layers
            .iter()
            .enumerate()
            .map(|(idx, written)| {
                let raw_size = Byte::from(written.raw.size).get_adjusted_unit(byte_unit::Unit::MB);
                let compressed_size = Byte::from(written.compressed.size).get_adjusted_unit(byte_unit::Unit::MB);
                HistoryBuilder::default()
                    .created(Utc::now().to_rfc3339())
                    .created_by(format!("splitter layer {idx}. Raw size: {raw_size:#.1}. Compressed size: {compressed_size:#.1}"))
                    .build()
                    .unwrap()
            })
            .collect();

        image_config.set_history(history);
        image_config.set_created(Some(Utc::now().to_rfc3339()));
        image_config.set_rootfs(
            RootFsBuilder::default()
                .typ("layers")
                .diff_ids(diff_ids)
                .build()?,
        );
        Ok(image_config)
    }

    pub fn create_manifest(&self, written_config: WrittenBlob) -> anyhow::Result<ImageManifest> {
        let layers: Vec<_> = self
            .layers
            .iter()
            .map(|layer| {
                Descriptor::new(
                    // MediaType::ImageLayerGzip,
                    MediaType::ImageLayerZstd,
                    layer.compressed.size as i64,
                    layer.compressed.prefixed_hash(),
                )
            })
            .collect();
        let config = Descriptor::new(
            MediaType::ImageConfig,
            written_config.raw.size as i64,
            written_config.raw.prefixed_hash(),
        );
        Ok(ImageManifestBuilder::default()
            .schema_version(2u32)
            .layers(layers)
            .config(config)
            .build()?)
    }

    pub fn create_index(&self, written_manifest: WrittenBlob) -> anyhow::Result<ImageIndex> {
        let builder = ImageIndexBuilder::default();
        let manifests = Descriptor::new(
            MediaType::ImageManifest,
            written_manifest.raw.size as i64,
            written_manifest.raw.prefixed_hash(),
        );
        let builder = builder.schema_version(2u32);
        let builder = builder.manifests([manifests]);
        Ok(builder.build()?)
    }
}
