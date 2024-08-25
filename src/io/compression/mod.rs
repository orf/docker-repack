use anyhow::bail;
use oci_spec::image::MediaType;
pub mod read;
pub mod tracked_encoder;
pub mod write;

#[derive(Debug, strum_macros::Display, Copy, Clone)]
pub enum CompressionType {
    ZStd,
    Gzip,
    None,
}

impl CompressionType {
    pub fn from_media_type(media_type: &MediaType) -> anyhow::Result<Self> {
        match media_type {
            MediaType::ImageLayer | MediaType::ImageLayerNonDistributable => Ok(Self::None),
            MediaType::ImageLayerGzip | MediaType::ImageLayerNonDistributableGzip => Ok(Self::Gzip),
            MediaType::ImageLayerZstd | MediaType::ImageLayerNonDistributableZstd => Ok(Self::ZStd),
            media_type => bail!("Unknown media type {media_type:?}"),
        }
    }
}
