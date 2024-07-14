use sha2::{Digest, Sha256};
use std::io::{self, BufWriter, Write};
use const_hex::Buffer;

#[derive(Debug)]
pub struct HashedCounterWriter<W: Write> {
    writer: BufWriter<W>,
    total_bytes_written: usize,
    hasher: Sha256,
}

impl<W: Write> HashedCounterWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer: BufWriter::new(writer),
            hasher: Sha256::new(),
            total_bytes_written: 0,
        }
    }

    pub fn finish(self) -> (BufWriter<W>, usize, Buffer<32>) {
        let mut buffer = Buffer::<32>::new();
        buffer.format(&self.hasher.finalize().into());
        (self.writer, self.total_bytes_written, buffer)
    }
}

impl<W: Write> Write for HashedCounterWriter<W> {
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

