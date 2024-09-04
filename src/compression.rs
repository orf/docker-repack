use anyhow::anyhow;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression as GzipCompression;
use std::io::{BufReader, BufWriter, Read, Write};
use zstd::{Decoder, Encoder};

#[derive(Debug, Clone, Copy, strum::Display, Eq, PartialEq)]
pub enum Compression {
    Raw,
    Gzip,
    Zstd,
}

impl Compression {
    pub fn new_reader<T: Read>(self, file: T) -> anyhow::Result<CompressedReader<'static, T>> {
        CompressedReader::new(self, file)
    }

    pub fn new_writer<T: Write + Sync + Send>(
        self,
        file: T,
        level: i32,
    ) -> anyhow::Result<CompressedWriter<'static, T>> {
        CompressedWriter::new(self, file, level)
    }
}

pub enum CompressedWriter<'a, T: Write + Sync + Send> {
    Raw(T),
    Gzip(BufWriter<GzEncoder<T>>),
    Zstd(BufWriter<Encoder<'a, T>>),
}

const DEFAULT_COMPRESSION_BUF_SIZE: usize = 1024 * 1024 * 25; // 25 mb

impl<'a, T: Write + Sync + Send> CompressedWriter<'a, T> {
    fn new(type_: Compression, file: T, level: i32) -> anyhow::Result<CompressedWriter<'a, T>> {
        match type_ {
            Compression::Raw => Ok(Self::Raw(file)),
            Compression::Gzip => Ok(Self::Gzip(BufWriter::with_capacity(
                DEFAULT_COMPRESSION_BUF_SIZE,
                GzEncoder::new(file, GzipCompression::new(level as u32)),
            ))),
            Compression::Zstd => {
                let encoder = Encoder::new(file, level)?;
                Ok(Self::Zstd(BufWriter::with_capacity(
                    DEFAULT_COMPRESSION_BUF_SIZE,
                    encoder,
                )))
            }
        }
    }

    pub fn tune_for_output_size(&mut self, size: u64) -> anyhow::Result<()> {
        if let CompressedWriter::Zstd(encoder) = self {
            let encoder = encoder.get_mut();
            encoder.set_pledged_src_size(Some(size))?;
            encoder.include_contentsize(true)?;
            encoder.include_checksum(false)?;
            encoder.long_distance_matching(true)?;
        }
        Ok(())
    }

    #[inline(always)]
    pub fn finish(self) -> anyhow::Result<()> {
        self.into_inner()?;
        Ok(())
    }

    #[inline(always)]
    pub fn into_inner(self) -> anyhow::Result<T> {
        match self {
            CompressedWriter::Raw(f) => Ok(f),
            CompressedWriter::Gzip(f) => {
                let inner = f.into_inner().map_err(|e| anyhow!("IntoInnerError {e}"))?;
                inner.finish().map_err(Into::into)
            }
            CompressedWriter::Zstd(f) => {
                let inner = f.into_inner().map_err(|e| anyhow!("IntoInnerError {e}"))?;
                inner.finish().map_err(Into::into)
            }
        }
    }
}

impl<T: Write + Sync + Send> Write for CompressedWriter<'_, T> {
    #[inline(always)]
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            CompressedWriter::Raw(f) => f.write(buf),
            CompressedWriter::Gzip(f) => f.write(buf),
            CompressedWriter::Zstd(f) => f.write(buf),
        }
    }

    #[inline(always)]
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            CompressedWriter::Raw(f) => f.flush(),
            CompressedWriter::Gzip(f) => f.flush(),
            CompressedWriter::Zstd(f) => f.flush(),
        }
    }
}

pub enum CompressedReader<'a, T: Read> {
    Raw(T),
    Gzip(GzDecoder<T>),
    Zstd(Decoder<'a, BufReader<T>>),
}

impl<'a, T: Read> CompressedReader<'a, T> {
    #[inline(always)]
    fn new(type_: Compression, file: T) -> anyhow::Result<CompressedReader<'a, T>> {
        match type_ {
            Compression::Raw => Ok(Self::Raw(file)),
            Compression::Gzip => Ok(Self::Gzip(GzDecoder::new(file))),
            Compression::Zstd => Ok(Self::Zstd(Decoder::new(file)?)),
        }
    }
}

impl<T: Read> Read for CompressedReader<'_, T> {
    #[inline(always)]
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            CompressedReader::Raw(f) => f.read(buf),
            CompressedReader::Gzip(f) => f.read(buf),
            CompressedReader::Zstd(f) => f.read(buf),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::compression::Compression;
    use flate2::write::GzEncoder;
    use std::io::{Read, Write};

    const CONTENT: &[u8] = b"hello world";

    #[test]
    fn raw_read() {
        let mut reader = Compression::Raw.new_reader(CONTENT).unwrap();
        let mut output = vec![];
        std::io::copy(&mut reader, &mut output).unwrap();
        assert_eq!(output, CONTENT);
    }

    #[test]
    fn gzip_read() {
        let mut content = GzEncoder::new(Vec::new(), flate2::Compression::default());
        content.write_all(CONTENT).unwrap();
        content.flush().unwrap();
        let compressed_content = content.finish().unwrap();
        let mut reader = Compression::Gzip.new_reader(compressed_content.as_slice()).unwrap();
        let mut output = vec![];
        std::io::copy(&mut reader, &mut output).unwrap();
        assert_eq!(output, CONTENT);
    }

    #[test]
    fn zstd_read() {
        let content = zstd::encode_all(CONTENT, 1).unwrap();
        let mut reader = Compression::Zstd.new_reader(content.as_slice()).unwrap();
        let mut output = vec![];
        std::io::copy(&mut reader, &mut output).unwrap();
        assert_eq!(output, CONTENT);
    }

    #[test]
    fn raw_write() {
        let mut writer = Compression::Raw.new_writer(vec![], 0).unwrap();
        writer.write_all(CONTENT).unwrap();
        let output = writer.into_inner().unwrap();
        assert_eq!(output, CONTENT);
    }
    #[test]
    fn gzip_write() {
        let mut writer = Compression::Gzip.new_writer(vec![], 1).unwrap();
        writer.write_all(CONTENT).unwrap();
        let compressed = writer.into_inner().unwrap();
        let mut s = vec![];
        flate2::read::GzDecoder::new(compressed.as_slice())
            .read_to_end(&mut s)
            .unwrap();
        assert_eq!(s, CONTENT);
    }

    #[test]
    fn zstd_write() {
        let mut writer = Compression::Zstd.new_writer(vec![], 1).unwrap();
        writer.write_all(CONTENT).unwrap();
        let compressed = writer.into_inner().unwrap();
        let s = zstd::decode_all(compressed.as_slice()).unwrap();
        assert_eq!(s, CONTENT);
    }
}
