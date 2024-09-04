use crate::compression::Compression;
use crate::input::Platform;
use crate::io_utils::WriteCounter;
use crate::output_image::layers::OutputLayer;
use crate::output_image::stats::WrittenImageStats;
use anyhow::Context;
use itertools::Itertools;
use oci_spec::image::{
    Descriptor, HistoryBuilder, ImageConfiguration, ImageIndexBuilder, ImageManifestBuilder, MediaType,
};
use serde::Serialize;
use sha2::Digest;
use std::fmt::{Debug, Display};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use tracing::debug;

pub struct WrittenLayer<'a> {
    pub(crate) layer: &'a OutputLayer<'a>,
    pub(crate) compressed_file_size: u64,
    pub(crate) raw_content_hash: String,
    pub(crate) compressed_content_hash: String,
}

pub struct OutputImageWriter {
    output_dir: PathBuf,
    blobs_dir: PathBuf,
    temp_dir: PathBuf,
}

impl Display for OutputImageWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "OutputImage {}", self.output_dir.display())
    }
}

impl OutputImageWriter {
    pub fn new(output_dir: PathBuf, temp_dir: PathBuf) -> anyhow::Result<Self> {
        let blobs_dir = output_dir.join("blobs").join("sha256");
        std::fs::create_dir_all(&blobs_dir).with_context(|| format!("Creating blobs directory {blobs_dir:?}"))?;
        std::fs::create_dir_all(&temp_dir).with_context(|| format!("Creating temp directory {temp_dir:?}"))?;
        Ok(Self {
            output_dir,
            blobs_dir,
            temp_dir,
        })
    }

    // #[instrument(skip_all, fields(self = %self, layers = %layers))]
    pub fn write_oci_image(
        &self,
        config: ImageConfiguration,
        mut written_layers: Vec<WrittenLayer>,
        platform: Platform,
    ) -> anyhow::Result<(u64, String, WrittenImageStats)> {
        // let mut written_layers = self.write_layers(layers, compression_level).context("Write layers")?;
        written_layers.sort_by_key(|l| (l.layer.type_, l.compressed_file_size));
        let (config_size, config_hash) = self.write_config(&config, &written_layers).context("Write config")?;
        self.build_manifest(config_size, config_hash, &written_layers, platform)
            .context("Build manifest")
    }

    pub fn write_image_index(self, manifests: &[(u64, String)]) -> anyhow::Result<()> {
        let index = manifests
            .iter()
            .map(|(size, hash)| Descriptor::new(MediaType::ImageManifest, *size as i64, format!("sha256:{hash}")))
            .collect_vec();
        let image_index = ImageIndexBuilder::default()
            .schema_version(2u32)
            .media_type(MediaType::ImageIndex)
            .manifests(index)
            .build()
            .context("ImageIndexBuilder Build")?;
        image_index.to_file_pretty(self.output_dir.join("index.json"))?;
        std::fs::write(self.output_dir.join("oci-layout"), "{\"imageLayoutVersion\":\"1.0.0\"}")?;
        Ok(())
    }

    fn build_manifest(
        &self,
        config_size: u64,
        config_hash: String,
        written_layers: &[WrittenLayer],
        platform: Platform,
    ) -> anyhow::Result<(u64, String, WrittenImageStats)> {
        let config_descriptor = Descriptor::new(
            MediaType::ImageConfig,
            config_size as i64,
            format!("sha256:{config_hash}"),
        );
        let layer_descriptors = written_layers
            .iter()
            .map(|l| {
                Descriptor::new(
                    MediaType::ImageLayerZstd,
                    l.compressed_file_size as i64,
                    format!("sha256:{}", l.compressed_content_hash),
                )
            })
            .collect_vec();

        let manifest = ImageManifestBuilder::default()
            .schema_version(2u32)
            .media_type(MediaType::ImageManifest)
            .config(config_descriptor)
            .layers(layer_descriptors)
            .build()
            .context("ImageManifestBuilder Build")?;
        let (manifest_size, manifest_hash) = self.add_json_to_blobs(&manifest).context("Write manifest to blobs")?;
        Ok((
            manifest_size,
            manifest_hash,
            WrittenImageStats::new(written_layers, platform),
        ))
    }

    fn write_config(&self, config: &ImageConfiguration, layers: &[WrittenLayer]) -> anyhow::Result<(u64, String)> {
        let created_at = chrono::Utc::now().to_rfc3339();
        let diff_ids = layers
            .iter()
            .map(|l| format!("sha256:{}", l.raw_content_hash))
            .collect_vec();
        let history: Result<Vec<_>, _> = layers
            .iter()
            .map(|l| {
                HistoryBuilder::default()
                    .author("docker-repack")
                    .created_by(l.layer.to_string())
                    .created(&created_at)
                    .empty_layer(false)
                    .build()
                    .with_context(|| format!("HistoryBuilder Build for layer {}", l.layer))
            })
            .collect();

        let mut config = config.clone();
        let root_fs = config.rootfs_mut();
        root_fs.set_diff_ids(diff_ids);
        config.set_history(history?);

        self.add_json_to_blobs(&config).context("Write config to blobs")
    }

    pub fn write_layer<'a>(&'a self, layer: &'a OutputLayer, compression_level: i32) -> anyhow::Result<WrittenLayer> {
        let mut hasher = sha2::Sha256::new();
        layer
            .to_writer_with_progress("Hashing raw layer", &mut hasher)
            .context("Hashing with to_writer")?;
        let digest: [u8; 32] = hasher.finalize().into();
        let raw_content_buffer: const_hex::Buffer<32> = const_hex::const_encode(&digest);
        let raw_content_hash = raw_content_buffer.as_str().to_string();

        let mut counter = WriteCounter::new();
        let writer = layer
            .to_writer_with_progress("Fetching raw size", &mut counter)
            .context("Write Counter")?;
        let raw_file_size = writer.written_bytes();

        let layer_path = self.temp_dir.join(format!("layer-{raw_content_hash}.tar.zst"));
        let layer_file = File::options()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&layer_path)
            .with_context(|| format!("Creating temp file {layer_path:?}"))?;
        let mut out = Compression::Zstd
            .new_writer(BufWriter::new(layer_file), compression_level)
            .context("Constructing CompressedWriter")?;
        out.tune_for_output_size(raw_file_size)?;
        layer
            .to_writer_with_progress("Compressing layer", &mut out)
            .context("to_writer")?;
        out.finish().context("Finishing compression")?;

        debug!("Layer compressed to {:?}", layer_path);
        let (compressed_file_size, compressed_content_hash) =
            self.add_path_to_blobs(&layer_path).context("Adding layer to blobs")?;
        Ok(WrittenLayer {
            layer,
            raw_content_hash,
            compressed_content_hash,
            compressed_file_size,
        })
    }

    fn add_json_to_blobs(&self, item: impl Serialize) -> anyhow::Result<(u64, String)> {
        let value = serde_json::to_string_pretty(&item)?;
        let (size, hash) = hash_reader(value.as_bytes())?;
        let path = self.blobs_dir.join(&hash);
        std::fs::write(&path, value)?;
        Ok((size, hash))
    }

    fn add_path_to_blobs(&self, input_path: impl AsRef<Path> + Debug) -> anyhow::Result<(u64, String)> {
        let (size, hash) = hash_file(&input_path).context("Hashing file")?;
        let path = self.blobs_dir.join(&hash);
        std::fs::rename(&input_path, &path).with_context(|| format!("Renaming {input_path:?} to {path:?}"))?;
        Ok((size, hash))
    }
}

fn hash_reader(mut content: impl Read) -> anyhow::Result<(u64, String)> {
    let mut hasher = sha2::Sha256::new();
    let compressed_file_size = std::io::copy(&mut content, &mut hasher).context("Copying bytes")?;
    let digest: [u8; 32] = hasher.finalize().into();
    let compressed_content_hash: const_hex::Buffer<32> = const_hex::const_encode(&digest);
    Ok((compressed_file_size, compressed_content_hash.as_str().to_string()))
}

fn hash_file(path: impl AsRef<Path> + Debug) -> anyhow::Result<(u64, String)> {
    let layer_file = File::options()
        .read(true)
        .open(&path)
        .with_context(|| format!("Opening {path:?} for reading"))?;
    hash_reader(BufReader::new(layer_file)).with_context(|| format!("Hashing {path:?}"))
}
