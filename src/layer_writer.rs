use crate::compact_layer::CompactLayer;
use crate::writer_utils::HashedCounterWriter;
use const_hex::Buffer;
use std::borrow::Cow;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read};
use std::path::{Path, PathBuf};
use flate2::Compression;
use tar::{Builder, Entry, EntryType};
use flate2::write::{GzDecoder, GzEncoder};
use crate::compression::GZIP_LEVEL;
use crate::image_writer::WrittenBlob;
// use zstd::Encoder;

// type TarBuilder<'a> = Builder<Encoder<'a, BufWriter<HashedCounterWriter<File>>>>;
// type TarBuilder = Builder<BufWriter<HashedCounterWriter<File>>>;
type TarBuilder = Builder<HashedCounterWriter<GzEncoder<HashedCounterWriter<File>>>>;

pub struct LayerWriter<'a> {
    pub path: PathBuf,
    pub layer: CompactLayer,
    // tar_builder: TarBuilder<'a>,
    tar_builder: TarBuilder,
    pub written: usize,
    include_symlinks: bool,
    a: std::marker::PhantomData<&'a ()>,
}

impl LayerWriter<'_> {
    pub fn create(path: PathBuf, layer: CompactLayer, include_symlinks: bool) -> Self {
        let writer = File::create(&path).unwrap();
        // Compressed hasher
        let writer = HashedCounterWriter::new(writer);
        let writer = GzEncoder::new(writer, GZIP_LEVEL);
        // let writer = Encoder::new(writer, 7).unwrap();
        // Raw hasher
        let writer = HashedCounterWriter::new(writer);
        let tar_builder = Builder::new(writer);
        Self {
            path,
            tar_builder,
            layer,
            written: 0,
            include_symlinks,
            a: std::marker::PhantomData,
        }
    }

    pub fn finish(self) -> anyhow::Result<WrittenBlob> {
        let inner = self.tar_builder.into_inner()?;
        let (inner, raw_bytes_written, raw_hash) = inner.finish();
        let inner = inner.into_inner()?;
        let inner = inner.finish()?;
        let (inner, compressed_bytes_written, compressed_hash) = inner.finish();
        let inner = inner.into_inner()?;
        drop(inner);
        Ok(WrittenBlob::new(
            self.path,
            compressed_hash,
            compressed_bytes_written as i64,
            raw_hash,
            raw_bytes_written as i64,
        ))
    }

    pub fn should_add_entry(
        &self,
        entry_type: EntryType,
        path: &Path,
        link_name: &Option<Cow<Path>>,
    ) -> bool {
        return match entry_type {
            EntryType::Regular => self.layer.paths.contains(path),
            EntryType::Link => {
                let name = link_name.as_ref().unwrap();
                self.layer.paths.contains(name.as_ref())
            }
            EntryType::Symlink => self.include_symlinks,
            EntryType::Directory => true,
            _ => false,
        };
    }

    pub fn add_entry(&mut self, path: &Path, entry: &Entry<BufReader<impl Read>>, data: &[u8]) {
        let entry_type = entry.header().entry_type();
        match entry_type {
            EntryType::Regular | EntryType::Directory => {
                self.tar_builder
                    .append_data(&mut entry.header().clone(), path, data)
                    .unwrap();
            }
            EntryType::Link | EntryType::Symlink => {
                let target_path = entry.link_name().unwrap().unwrap();
                self.tar_builder
                    .append_link(&mut entry.header().clone(), path, target_path)
                    .unwrap();
            }
            _ => {
                panic!("Unsupported entry type {entry_type:?}")
            }
        }
        self.written += 1;
    }
}
