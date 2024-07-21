use crate::image_parser::image::LayerID;
use crate::image_parser::path_filter::PathFilter;
use crate::image_parser::{HashAndSize, HashedWriter};
use std::fs::File;
use std::io::{BufWriter, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tar::{Builder, Entry, EntryType};

pub struct LayerWriter<'a> {
    name: &'static str,
    path: PathBuf,
    writer: Mutex<Builder<BufWriter<HashedWriter<File>>>>,
    filter: PathFilter<'a>,
    entries: AtomicUsize,
}

impl<'a> LayerWriter<'a> {
    pub fn create_layer(
        name: &'static str,
        path: PathBuf,
        filter: PathFilter<'a>,
    ) -> anyhow::Result<LayerWriter<'a>> {
        let writer = HashedWriter::new(File::create(&path)?);
        let writer = Builder::new(BufWriter::new(writer));
        Ok(LayerWriter {
            name,
            path,
            writer: Mutex::new(writer),
            filter,
            entries: 0.into(),
        })
    }

    pub fn contains_path(&self, id: LayerID, path: &str) -> bool {
        self.filter.contains_path(id, path)
    }

    pub fn write_entry(&self, entry: &Entry<impl Read>, data: &[u8]) -> anyhow::Result<()> {
        let entry_type = entry.header().entry_type();
        let mut header = entry.header().clone();
        let path = entry.path()?;
        let mut writer = self.writer.lock().unwrap();
        match entry_type {
            EntryType::Regular | EntryType::Directory => {
                writer.append_data(&mut header, path, data)?
            }
            EntryType::Link | EntryType::Symlink => {
                let target_path = entry.link_name()?.unwrap();
                writer.append_link(&mut header, path, target_path)?;
            }
            _ => {
                panic!("Unsupported entry type {entry_type:?}")
            }
        }
        self.entries.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    pub fn finish(self) -> anyhow::Result<WrittenLayer> {
        let inner = self.writer.into_inner().unwrap();
        let inner = inner.into_inner()?;
        let inner = inner.into_inner()?;
        let (_, hash) = inner.into_inner();
        Ok(WrittenLayer {
            path: self.path,
            hash,
            entries: self.entries.load(Ordering::SeqCst),
        })
    }
}

#[derive(Debug)]
pub struct WrittenLayer {
    pub path: PathBuf,
    pub hash: HashAndSize,
    pub entries: usize,
}
