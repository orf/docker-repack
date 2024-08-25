use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use zstd::stream::write::Encoder;

#[derive(Debug, Default)]
pub struct TrackedEncoder {
    counter: AtomicUsize,
}

impl TrackedEncoder {
    pub fn create_encoder(&self) -> Encoder<TrackedWriter> {
        let tracker = TrackedWriter {
            counter: &self.counter,
        };
        Encoder::new(tracker, 1).unwrap()
    }

    pub fn bytes_written(&self) -> usize {
        self.counter.load(Ordering::SeqCst)
    }
}

pub struct TrackedWriter<'a> {
    counter: &'a AtomicUsize,
}

impl<'a> Write for TrackedWriter<'a> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.counter.fetch_add(buf.len(), Ordering::Relaxed);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let counter = AtomicUsize::new(0);
        let mut writer = TrackedWriter { counter: &counter };
        let contents = b"hello world";
        for _ in 0..10 {
            writer.write(contents).unwrap();
        }
        assert_eq!(counter.load(Ordering::SeqCst), contents.len() * 10);
    }

    #[test]
    fn it_works_zstd() {
        let tracker = TrackedEncoder::default();
        let mut encoder = tracker.create_encoder();
        let contents = b"hello world";
        for _ in 0..1000000 {
            encoder.write_all(contents).unwrap();
        }

        encoder.flush().unwrap();
        assert_eq!(tracker.bytes_written(), 1024);
    }
}
