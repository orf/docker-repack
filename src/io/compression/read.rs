use crate::io::compression::CompressionType;
use flate2::bufread::GzDecoder;
use std::io::{BufRead, BufReader};

pub fn decompress_reader(
    compression_type: CompressionType,
    reader: impl BufRead + 'static,
) -> anyhow::Result<Box<dyn BufRead>> {
    match compression_type {
        CompressionType::ZStd => Ok(Box::new(BufReader::new(zstd::stream::Decoder::new(
            reader,
        )?))),
        CompressionType::Gzip => Ok(Box::new(BufReader::new(GzDecoder::new(reader)))),
        CompressionType::None => Ok(Box::new(reader)),
    }
}
