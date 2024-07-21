use const_hex::Buffer;
use sha2::{Digest, Sha256};
use std::io;
use std::io::Write;

#[derive(Debug)]
pub struct HashAndSize {
    hash: String,
    pub size: u64,
}

impl HashAndSize {
    pub fn prefixed_hash(&self) -> String {
        format!("sha256:{}", self.hash.as_str())
    }

    pub fn raw_hash(&self) -> &str {
        self.hash.as_str()
    }
}

#[derive(Debug)]
pub struct HashedWriter<W: Write> {
    writer: W,
    total_bytes_written: usize,
    hasher: Sha256,
}

impl<W: Write> HashedWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            hasher: Sha256::new(),
            total_bytes_written: 0,
        }
    }

    pub fn into_inner(self) -> (W, HashAndSize) {
        let mut buffer = Buffer::<32>::new();
        buffer.format(&self.hasher.finalize().into());
        (
            self.writer,
            HashAndSize {
                size: self.total_bytes_written as u64,
                hash: buffer.to_string(),
            },
        )
    }
}

impl<W: Write> Write for HashedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let bytes_written = self.writer.write(buf)?;
        self.hasher.update(&buf[..bytes_written]);
        self.total_bytes_written += bytes_written;
        Ok(bytes_written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
