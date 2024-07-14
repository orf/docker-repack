use std::io;
use byte_unit::Byte;
use flate2::Compression;
use flate2::write::GzEncoder;
use std::io::Write;

pub const ZSTD_LEVEL: i32 = 0;

pub const GZIP_LEVEL: Compression = Compression::fast();

pub fn calculate_compressed_size_zstd(data: &[u8], buffer: &mut Vec<u8>) -> io::Result<Byte> {
    if data.is_empty() {
        return Ok(Byte::from(0u64));
    }
    buffer.clear();
    match zstd::stream::copy_encode(data, &mut *buffer, ZSTD_LEVEL) {
        Ok(_) => Ok(Byte::from(buffer.len() as u64)),
        Err(e) => Err(e),
    }
}


pub fn calculate_compressed_size_gzip(data: &[u8], buffer: &mut Vec<u8>) -> io::Result<Byte> {
    if data.is_empty() {
        return Ok(Byte::from(0u64));
    }
    buffer.clear();
    let mut encoder = GzEncoder::new(&mut *buffer, GZIP_LEVEL);
    encoder.write_all(data)?;
    let data = encoder.finish()?;
    Ok(Byte::from(data.len() as u64))
    // match zstd::stream::copy_encode(data, &mut *buffer, ZSTD_LEVEL) {
    //     Ok(_) => Ok(Byte::from(buffer.len() as u64)),
    //     Err(e) => Err(e),
    // }
}