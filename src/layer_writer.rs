use crate::compact_layer::CompactLayer;
use crate::compression::ZSTD_LEVEL;
use crate::image_writer::WrittenBlob;
use crate::writer_utils::HashedCounterWriter;
use anyhow::bail;
use std::borrow::Cow;
use std::collections::HashSet;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use byte_unit::Byte;
use tar::{Builder, Entry, EntryType};
use zstd::Encoder as ZstEncoder;

// type TarBuilder<'a> = Builder<Encoder<'a, BufWriter<HashedCounterWriter<File>>>>;
// type TarBuilder = Builder<BufWriter<HashedCounterWriter<File>>>;
// type TarBuilder = Builder<HashedCounterWriter<GzEncoder<HashedCounterWriter<File>>>>;
type TarBuilder<'a> = Builder<HashedCounterWriter<ZstEncoder<'a, HashedCounterWriter<File>>>>;

pub struct LayerWriter<'a> {
    pub path: PathBuf,
    pub paths: HashSet<PathBuf>,
    // tar_builder: TarBuilder<'a>,
    tar_builder: TarBuilder<'a>,
    pub written: usize,
    is_directory_layer: bool,
    pub total_size: Byte,
    a: std::marker::PhantomData<&'a ()>,
}

fn create_writer<'a>(path: &PathBuf) -> TarBuilder<'a> {
    let writer = File::create(path).unwrap();
    let writer = HashedCounterWriter::new(writer);
    // let writer = GzEncoder::new(writer, GZIP_LEVEL);
    let writer = ZstEncoder::new(writer, ZSTD_LEVEL).unwrap();
    let writer = HashedCounterWriter::new(writer);

    Builder::new(writer)
}

impl LayerWriter<'_> {
    pub fn create(path: PathBuf, layer: CompactLayer) -> Self {
        let tar_builder = create_writer(&path.clone());
        Self {
            path,
            tar_builder,
            paths: layer.paths,
            written: 0,
            is_directory_layer: false,
            total_size: layer.total_size,
            a: std::marker::PhantomData,
        }
    }

    pub fn create_directory_layer(path: PathBuf) -> Self {
        let tar_builder = create_writer(&path.clone());
        Self {
            path,
            tar_builder,
            paths: HashSet::new(),
            written: 0,
            is_directory_layer: true,
            total_size: Byte::from(0u64),
            a: std::marker::PhantomData,
        }
    }

    pub fn finish(self) -> anyhow::Result<WrittenBlob> {
        let inner = self.tar_builder.into_inner()?;
        let (inner, raw_bytes_written, raw_hash) = inner.finish();
        let inner = match inner.into_inner() {
            Ok(v) => v,
            Err(_) => {
                bail!("Failed to finish writing layer")
            }
        };
        let inner = inner.finish()?;
        let (inner, compressed_bytes_written, compressed_hash) = inner.finish();
        let inner = inner.into_inner()?;
        drop(inner);
        Ok(WrittenBlob::new(
            self.path,
            compressed_hash,
            compressed_bytes_written as u64,
            raw_hash,
            raw_bytes_written as u64,
        ))
    }

    pub fn should_add_entry(
        &self,
        entry_type: EntryType,
        path: &Path,
        link_name: &Option<Cow<Path>>,
    ) -> bool {
        let is_symlink_or_dir = matches!(entry_type, EntryType::Symlink | EntryType::Directory);
        if self.is_directory_layer {
            return is_symlink_or_dir;
        }
        return match entry_type {
            EntryType::Regular => self.paths.contains(path),
            EntryType::Link => {
                let name = link_name.as_ref().unwrap();
                self.paths.contains(name.as_ref())
            }
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

impl Display for LayerWriter<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let compressed_size = Byte::from(self.total_size).get_adjusted_unit(byte_unit::Unit::MB);
        write!(
            f,
            "{:?} - is_directory_layer: {} - size: {:#.1}",
            self.path, self.is_directory_layer, compressed_size
        )
    }
}
