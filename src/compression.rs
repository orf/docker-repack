use byte_unit::Byte;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io;
use std::io::Write;

pub const ZSTD_LEVEL: i32 = 9;

pub const GZIP_LEVEL: Compression = Compression::best();

pub fn calculate_compressed_size(data: &[u8], buffer: &mut Vec<u8>) -> io::Result<Byte> {
    buffer.clear();
    if data.is_empty() {
        return Ok(Byte::from(0u64));
    }
    calculate_compressed_size_zstd(data, buffer)
    // calculate_compressed_size_gzip(data, buffer)
}

pub fn calculate_compressed_size_zstd(data: &[u8], buffer: &mut Vec<u8>) -> io::Result<Byte> {
    match zstd::stream::copy_encode(data, &mut *buffer, ZSTD_LEVEL) {
        Ok(_) => Ok(Byte::from(buffer.len() as u64)),
        Err(e) => Err(e),
    }
}

pub fn calculate_compressed_size_gzip(data: &[u8], buffer: &mut Vec<u8>) -> io::Result<Byte> {
    let mut encoder = GzEncoder::new(&mut *buffer, GZIP_LEVEL);
    let data = encoder.finish()?;
    Ok(Byte::from(data.len() as u64))
}
