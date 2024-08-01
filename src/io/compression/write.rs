use crate::io::hashed_writer::{HashAndSize, HashedWriter};
use crate::io::new_bufwriter;
use crate::io::utils::progress_reader;
use indicatif::MultiProgress;
use std::fs::File;
use std::io;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;
use zstd::zstd_safe::CompressionLevel;
use zstd::{zstd_safe, Encoder};

pub fn compress_stream(
    progress: &MultiProgress,
    input_size: u64,
    reader: impl BufRead,
    output: impl Write,
    message: String,
    level: CompressionLevel,
) -> anyhow::Result<HashAndSize> {
    let hash_writer = HashedWriter::new(output);
    let hash_writer = BufWriter::with_capacity(zstd_safe::CCtx::in_size() * 2, hash_writer);
    let mut encoder = Encoder::new(hash_writer, level)?;

    let reader = BufReader::with_capacity(zstd_safe::CCtx::in_size(), reader);
    let mut progress_reader = progress_reader(progress, input_size, reader, message);

    encoder.include_contentsize(true)?;
    encoder.long_distance_matching(true)?;
    encoder.set_pledged_src_size(Some(input_size))?;

    std::io::copy(&mut progress_reader, &mut encoder)?;

    let hash_writer = encoder.finish()?;
    let hash_writer = match hash_writer.into_inner() {
        Ok(e) => e,
        Err(_) => {
            panic!("shit")
        }
    };
    let (mut content, hash_and_size) = hash_writer.into_inner();
    content.flush()?;
    Ok(hash_and_size)
}

pub fn compress_file(
    progress: &MultiProgress,
    output_path: &Path,
    input_path: &Path,
    level: CompressionLevel,
    message: String,
) -> anyhow::Result<HashAndSize> {
    let input_file = File::open(input_path)?;
    let input_size = input_file.metadata()?.len();
    let output_file = new_bufwriter(File::create(output_path)?);
    let input_file = crate::io::new_mmap(input_file, true)?;
    let hash_and_size = compress_stream(
        progress,
        input_size,
        io::Cursor::new(input_file),
        output_file,
        message,
        level,
    )?;
    Ok(hash_and_size)
}
