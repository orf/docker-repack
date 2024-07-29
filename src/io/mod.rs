use memmap2::{Advice, Mmap};
use std::io::{BufWriter, Write};

mod compression;
pub mod hashed_writer;
pub mod image;
pub mod layer;
mod utils;

const DEFAULT_BUFFER_SIZE: usize = 4 * 1024 * 1024;

fn new_bufwriter<T: Write>(item: T) -> BufWriter<T> {
    BufWriter::with_capacity(DEFAULT_BUFFER_SIZE, item)
}

fn new_mmap(file: std::fs::File, sequential: bool) -> anyhow::Result<Mmap> {
    let reader = unsafe { memmap2::MmapOptions::new().map(&file) }?;
    #[cfg(unix)]
    {
        if sequential {
            reader.advise(Advice::Sequential)?;
        } else {
            reader.advise(Advice::Random)?;
        }
        reader.advise(Advice::WillNeed)?;
    }

    Ok(reader)
}
