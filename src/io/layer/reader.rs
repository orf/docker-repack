use crate::io::compression::read::decompress_reader;
use crate::io::compression::CompressionType;
use crate::io::image::reader::SourceLayerID;
use crate::io::{new_bufwriter, new_mmap, utils};
use crate::tar_item::TarItem;
use anyhow::Context;
use indicatif::MultiProgress;
use memmap2::Mmap;
use std::fs::File;
use std::io;
use std::io::{BufRead, Cursor, Seek};
use std::path::PathBuf;
use tar::{Archive, Entry};

#[derive(Debug)]
pub struct CompressedLayer {
    pub id: SourceLayerID,
    pub path: PathBuf,
    pub size: u64,
    pub compressed_size: u64,
    pub compression: CompressionType,
}

impl CompressedLayer {
    pub fn get_progress_reader(
        &self,
        progress: &MultiProgress,
        message_prefix: &'static str,
    ) -> anyhow::Result<impl BufRead> {
        let file = File::open(&self.path)?;
        let file = new_mmap(file, true)?;
        let reader = utils::progress_reader(
            progress,
            self.size,
            io::Cursor::new(file),
            format!("{message_prefix} {}", self.id),
        );
        let decompress_reader = decompress_reader(self.compression, reader)?;
        Ok(decompress_reader)
    }

    pub fn decompress(self, progress: &MultiProgress, output_path: PathBuf) -> anyhow::Result<DecompressedLayer> {
        let mut reader = self.get_progress_reader(progress, "Decompressing layer")?;
        let mut writer = new_bufwriter(
            File::create(&output_path)
                .with_context(|| format!("Error decompressing layer {:?} to file {output_path:?}", self.path))?,
        );
        let size = std::io::copy(&mut reader, &mut writer)?;
        Ok(DecompressedLayer {
            id: self.id,
            path: output_path,
            size: size as u64,
        })
    }
}

#[derive(Debug)]
pub struct DecompressedLayer {
    pub id: SourceLayerID,
    pub path: PathBuf,
    pub size: u64,
}

impl DecompressedLayer {
    pub fn get_reader(&self) -> anyhow::Result<Cursor<Mmap>> {
        let file = File::open(&self.path)?;
        Ok(io::Cursor::new(new_mmap(file, true)?))
    }

    pub fn get_raw_mmap(&self) -> anyhow::Result<Mmap> {
        let file = File::open(&self.path)?;
        new_mmap(file, false)
    }

    pub fn get_progress_reader(
        &self,
        progress: &MultiProgress,
        message_prefix: &'static str,
    ) -> anyhow::Result<impl BufRead + Seek> {
        let file = self.get_reader()?;
        let reader = utils::progress_reader(progress, self.size, file, format!("{message_prefix} {}", self.id));
        Ok(reader)
    }

    pub fn get_seekable_reader(&self) -> anyhow::Result<SeekableLayerReader> {
        Ok(SeekableLayerReader {
            mmap: self.get_raw_mmap()?,
        })
    }
}

#[derive(Debug)]
pub struct SeekableLayerReader {
    mmap: Mmap,
}

impl SeekableLayerReader {
    pub fn open_position(&self, position: usize) -> SeekableArchive {
        SeekableArchive::new(&self.mmap[position..])
    }

    pub fn get_data_slice(&self, item: &TarItem) -> &[u8] {
        let start = item.data_position as usize;
        let end = start + item.size as usize;
        &self.mmap[start..end]
    }
}

pub struct SeekableArchive<'a> {
    archive: Archive<Cursor<&'a [u8]>>,
}

impl<'a> SeekableArchive<'a> {
    pub fn new(memory_slice: &'a [u8]) -> Self {
        let archive = Archive::new(Cursor::new(memory_slice));
        Self { archive }
    }

    pub fn read_entry(&mut self) -> anyhow::Result<Entry<Cursor<&'a [u8]>>> {
        let mut entries = self.archive.entries()?;
        let next = entries.next().unwrap()?;
        Ok(next)
    }
}
