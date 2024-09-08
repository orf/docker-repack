use crate::compression::Compression;
use crate::input::layers::InputLayer;
use itertools::Itertools;
use oci_client::manifest::{
    IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE, IMAGE_DOCKER_LAYER_TAR_MEDIA_TYPE, IMAGE_LAYER_GZIP_MEDIA_TYPE,
    IMAGE_LAYER_MEDIA_TYPE, IMAGE_LAYER_NONDISTRIBUTABLE_GZIP_MEDIA_TYPE, IMAGE_LAYER_NONDISTRIBUTABLE_MEDIA_TYPE,
};
use oci_spec::image::{ImageConfiguration, MediaType};
use std::fmt::{Display, Formatter, Write};
use std::hash::Hash;
use std::io::Read;

pub mod layers;
pub mod local_image;
pub mod remote_image;

const IMAGE_DOCKER_LAYER_ZSTD_MEDIA_TYPE: &str = "application/vnd.docker.image.rootfs.diff.tar.zstd";
const IMAGE_LAYER_ZSTD_MEDIA_TYPE: &str = "application/vnd.oci.image.layer.v1.tar+zstd";
const IMAGE_LAYER_NONDISTRIBUTABLE_ZSTD_MEDIA_TYPE: &str =
    "application/vnd.oci.image.layer.nondistributable.v1.tar+zstd";

pub fn get_layer_media_type(value: &str) -> Option<MediaType> {
    match value {
        IMAGE_DOCKER_LAYER_TAR_MEDIA_TYPE | IMAGE_LAYER_MEDIA_TYPE | IMAGE_LAYER_NONDISTRIBUTABLE_MEDIA_TYPE => {
            Some(MediaType::ImageLayer)
        }
        IMAGE_DOCKER_LAYER_GZIP_MEDIA_TYPE
        | IMAGE_LAYER_GZIP_MEDIA_TYPE
        | IMAGE_LAYER_NONDISTRIBUTABLE_GZIP_MEDIA_TYPE => Some(MediaType::ImageLayerGzip),
        IMAGE_DOCKER_LAYER_ZSTD_MEDIA_TYPE
        | IMAGE_LAYER_ZSTD_MEDIA_TYPE
        | IMAGE_LAYER_NONDISTRIBUTABLE_ZSTD_MEDIA_TYPE => Some(MediaType::ImageLayerZstd),
        _ => None,
    }
}

#[derive(Clone, Eq, PartialEq)]
pub struct Platform {
    config: ImageConfiguration,
}

impl Platform {
    pub fn file_key(&self) -> anyhow::Result<String> {
        let mut f = String::new();
        f.write_fmt(format_args!("{}-{}", self.config.os(), self.config.architecture()))?;
        if let Some(variant) = self.config.variant() {
            f.write_fmt(format_args!("-{}", variant))?;
        }
        if let Some(os_version) = self.config.os_version() {
            f.write_fmt(format_args!("-{}", os_version))?;
        }
        if let Some(os_features) = self.config.os_features() {
            for feature in os_features {
                f.write_fmt(format_args!("-{}", feature))?;
            }
        }
        Ok(f)
    }
}

impl Display for Platform {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.config.os(), self.config.architecture())?;
        if let Some(variant) = self.config.variant() {
            write!(f, "/{}", variant)?;
        }
        Ok(())
    }
}

pub trait InputImage: Display + Sized + Send + Sync + Hash + Eq + PartialEq {
    fn image_digest(&self) -> String;

    fn platform(&self) -> Platform {
        let config = self.config().clone();
        Platform { config }
    }

    fn layers_from_manifest(
        &self,
    ) -> anyhow::Result<impl ExactSizeIterator<Item = anyhow::Result<InputLayer<impl Read>>>>;

    fn config(&self) -> &ImageConfiguration;

    fn layers(&self) -> anyhow::Result<Vec<(MediaType, String)>>;

    fn layers_with_compression(&self) -> anyhow::Result<impl ExactSizeIterator<Item = (Compression, String)>> {
        let iterator = self
            .layers()?
            .into_iter()
            .filter_map(|(media_type, digest)| match media_type {
                MediaType::ImageLayer | MediaType::ImageLayerNonDistributable => Some((Compression::Raw, digest)),
                MediaType::ImageLayerGzip | MediaType::ImageLayerNonDistributableGzip => {
                    Some((Compression::Gzip, digest))
                }
                MediaType::ImageLayerZstd | MediaType::ImageLayerNonDistributableZstd => {
                    Some((Compression::Zstd, digest))
                }
                _ => None,
            })
            .rev();
        Ok(iterator.collect_vec().into_iter())
    }
}
