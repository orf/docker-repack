use crate::input::layers::InputLayer;
use crate::input::InputImage;
use crate::platform_matcher::PlatformMatcher;
use crate::progress;
use anyhow::{bail, Context};
use oci_spec::image::{Descriptor, Digest, ImageConfiguration, ImageIndex, ImageManifest, MediaType};
use std::fmt::{Debug, Display, Formatter};
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use tracing::{debug, instrument, warn};

pub struct LocalOciImage {
    blob_directory: PathBuf,
    manifest: ImageManifest,
    image_config: ImageConfiguration,
}

impl PartialEq for LocalOciImage {
    fn eq(&self, other: &Self) -> bool {
        self.image_digest() == other.image_digest()
    }
}

impl Eq for LocalOciImage {}

impl Hash for LocalOciImage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let digest = self.image_digest();
        digest.digest().hash(state);
        digest.algorithm().as_ref().hash(state);
    }
}

impl Debug for LocalOciImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.manifest.config().digest().digest())
    }
}

impl Display for LocalOciImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.platform().fmt(f)
    }
}

fn get_blob_path(blob_directory: &Path, descriptor: &Descriptor) -> PathBuf {
    let digest = descriptor.digest();
    blob_directory.join(digest.digest())
}

fn read_blob_image_manifest(blob_directory: &Path, descriptor: &Descriptor) -> anyhow::Result<ImageManifest> {
    let digest_path = get_blob_path(blob_directory, descriptor);
    ImageManifest::from_file(&digest_path).with_context(|| format!("Error reading image manifest from {digest_path:?}"))
}

fn read_blob_image_index(blob_directory: &Path, descriptor: &Descriptor) -> anyhow::Result<ImageIndex> {
    let digest_path = get_blob_path(blob_directory, descriptor);
    ImageIndex::from_file(&digest_path).with_context(|| format!("Error reading image index from {digest_path:?}"))
}

impl LocalOciImage {
    #[instrument(name = "load_images")]
    pub fn from_oci_directory(
        directory: impl AsRef<Path> + Debug,
        platform_matcher: &PlatformMatcher,
    ) -> anyhow::Result<Vec<Self>> {
        let directory = directory.as_ref();
        let blob_directory = directory.join("blobs").join("sha256");

        let index_path = directory.join("index.json");
        let manifest_path = directory.join("manifest.json");

        if index_path.exists() {
            debug!("Reading index from {index_path:?}");
            let index = ImageIndex::from_file(&index_path)
                .with_context(|| format!("Error reading index from {index_path:?}"))?;
            let mut images = vec![];
            let manifest_iterator = progress::progress_iter("Reading Manifests", index.manifests().iter());
            for manifest_descriptor in manifest_iterator {
                if !platform_matcher.matches_oci_spec_platform(manifest_descriptor.platform().as_ref()) {
                    continue;
                }
                match manifest_descriptor.media_type() {
                    MediaType::ImageManifest => {
                        debug!("Reading image manifest from {}", manifest_descriptor.digest());
                        let manifest = read_blob_image_manifest(&blob_directory, manifest_descriptor)
                            .context("Reading manifest")?;
                        let img = Self::from_image_manifest(manifest, blob_directory.clone())
                            .context("Constructing LocalOciImage")?;
                        images.push(img);
                    }
                    MediaType::ImageIndex => {
                        debug!("Reading image index from {}", manifest_descriptor.digest());
                        let index =
                            read_blob_image_index(&blob_directory, manifest_descriptor).context("Reading index")?;
                        images.extend(
                            Self::from_image_index(index, blob_directory.clone(), platform_matcher)
                                .context("Parsing image index")?,
                        );
                    }
                    _ => {
                        warn!("Skipping unknown media type {}", manifest_descriptor.media_type());
                    }
                }
            }
            Ok(images)
        } else if manifest_path.exists() {
            debug!("Reading manifest from {manifest_path:?}");
            let manifest = ImageManifest::from_file(&manifest_path)
                .with_context(|| format!("Error reading manifest from {manifest_path:?}"))?;
            let img = Self::from_image_manifest(manifest, blob_directory).context("Constructing LocalOciImage")?;
            Ok(vec![img])
        } else {
            bail!("No manifest or index found in {directory:?}");
        }
    }

    fn from_image_index(
        index: ImageIndex,
        blob_directory: PathBuf,
        platform_matcher: &PlatformMatcher,
    ) -> anyhow::Result<Vec<Self>> {
        let mut images = vec![];
        for manifest_descriptor in index.manifests() {
            if !platform_matcher.matches_oci_spec_platform(manifest_descriptor.platform().as_ref()) {
                continue;
            }
            let manifest = read_blob_image_manifest(&blob_directory, manifest_descriptor)?;
            let img = Self::from_image_manifest(manifest, blob_directory.clone())
                .with_context(|| format!("Constructing LocalOciImage for {}", manifest_descriptor.digest()))?;
            images.push(img);
        }
        Ok(images)
    }

    fn from_image_manifest(manifest: ImageManifest, blob_directory: PathBuf) -> anyhow::Result<Self> {
        let config_descriptor = manifest.config();
        let config_digest = config_descriptor.digest();
        let config_path = blob_directory.join(config_digest.digest());
        let image_config = ImageConfiguration::from_file(&config_path)
            .with_context(|| format!("Error reading image configuration from {config_path:?}"))?;
        Ok(Self {
            blob_directory,
            manifest,
            image_config,
        })
    }
}

impl InputImage for LocalOciImage {
    fn image_digest(&self) -> Digest {
        let digest = self.manifest.config().digest();
        digest.clone()
    }

    fn layers_from_manifest(
        &self,
    ) -> anyhow::Result<impl ExactSizeIterator<Item = anyhow::Result<InputLayer<impl Read>>>> {
        Ok(self.layers_with_compression()?.map(|(compression, digest)| {
            let path = self.blob_directory.join(digest.digest());
            let file = File::open(&path).with_context(|| format!("Error reading input layer from {path:?}"))?;
            let reader = compression.new_reader(file)?;
            InputLayer::new(digest, reader)
        }))
    }

    fn config(&self) -> &ImageConfiguration {
        &self.image_config
    }

    fn layers(&self) -> anyhow::Result<Vec<(MediaType, Digest)>> {
        Ok(self
            .manifest
            .layers()
            .iter()
            .map(|d| {
                let stripped_digest = d.digest();
                (d.media_type().clone(), stripped_digest.clone())
            })
            .collect())
    }
}
