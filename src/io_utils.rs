use std::io::Write;

pub struct WriteCounter {
    count: u64,
}

impl WriteCounter {
    pub fn new() -> Self {
        Self { count: 0 }
    }

    pub fn written_bytes(&self) -> u64 {
        self.count
    }
}

impl Write for WriteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.count += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
