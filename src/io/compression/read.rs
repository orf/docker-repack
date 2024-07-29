use crate::io::compression::CompressionType;
use flate2::read::GzDecoder;
use std::io::Read;

pub fn decompress_reader(
    compression_type: CompressionType,
    reader: impl Read + 'static,
) -> anyhow::Result<Box<dyn Read>> {
    match compression_type {
        CompressionType::ZStd => Ok(Box::new(zstd::stream::Decoder::new(reader)?)),
        CompressionType::Gzip => Ok(Box::new(GzDecoder::new(reader))),
        CompressionType::None => Ok(Box::new(reader)),
    }
}
