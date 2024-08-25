use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use zstd::stream::write::AutoFinishEncoder;
use zstd::Encoder;

pub struct TrackedEncoderWriter<'a> {
    counter: Arc<AtomicU64>,
    stream_encoder: AutoFinishEncoder<'a, TrackedWriter<Arc<AtomicU64>>>,
}

impl TrackedEncoderWriter<'_> {
    pub fn new() -> anyhow::Result<Self> {
        let counter = Arc::new(AtomicU64::new(0));
        let tracker = TrackedWriter {
            counter: counter.clone(),
        };
        let encoder = Encoder::new(tracker, 1)?;
        Ok(TrackedEncoderWriter {
            stream_encoder: encoder.auto_finish(),
            counter: counter.clone(),
        })
    }

    pub fn bytes_written(&self) -> u64 {
        self.counter.load(Ordering::SeqCst)
    }

    pub fn copy_item(&mut self, mut reader: &[u8]) -> std::io::Result<u64> {
        std::io::copy(&mut reader, &mut self.stream_encoder)
    }

    pub fn flush(&mut self) -> std::io::Result<()> {
        self.stream_encoder.flush()
    }
}

pub struct TrackedWriter<T> {
    counter: T,
}

impl<T> TrackedWriter<T> {
    pub fn into_counter(self) -> T {
        self.counter
    }
}

impl Write for &mut TrackedWriter<u64> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.counter += buf.len() as u64;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Write for TrackedWriter<Arc<AtomicU64>> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.counter.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
