use crate::image_parser::{HashAndSize, HashedWriter};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;
use std::io::{BufWriter, Read};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tar::{Builder, Entry, EntryType};

pub struct LayerWriter {
    path: PathBuf,
    archive_writer: Mutex<Builder<BufWriter<HashedWriter<File>>>>,
    index_writer: Mutex<BufWriter<File>>,
    entries: AtomicUsize,
    expected_entries: usize,
}

impl LayerWriter {
    pub fn create_layer(path: PathBuf, expected_entries: usize) -> anyhow::Result<LayerWriter> {
        let writer = HashedWriter::new(File::create(&path)?);
        let writer = Builder::new(BufWriter::new(writer));
        let index_writer = BufWriter::new(File::create(path.with_extension("index.txt"))?);
        Ok(LayerWriter {
            path,
            archive_writer: Mutex::new(writer),
            index_writer: Mutex::new(index_writer),
            entries: 0.into(),
            expected_entries,
        })
    }

    pub fn write_entry(&self, entry: &Entry<impl Read>, data: &[u8]) -> anyhow::Result<()> {
        let entry_type = entry.header().entry_type();
        let link_name_binding = entry.link_name()?;
        let link_name = link_name_binding
            .as_ref()
            .map(|p| p.to_str().unwrap())
            .unwrap_or_default();
        let mut header = entry.header().clone();
        let path = entry.path()?;
        let expected_size = entry.size();

        let prev = self.entries.fetch_add(1, Ordering::Relaxed);
        let mut index_writer = self.index_writer.lock().unwrap();
        writeln!(
            index_writer,
            "{prev:>10} {entry_type:>20?} - len: {:>10} expected: {:>10} path: {} link: {}",
            data.len(),
            expected_size,
            path.display(),
            link_name
        )?;
        drop(index_writer);

        // assert_eq!(data.len(), expected_size as usize);

        let mut writer = self.archive_writer.lock().unwrap();
        match entry_type {
            EntryType::Regular | EntryType::Directory => {
                writer.append_data(&mut header, &path, data)?
            }
            EntryType::Link | EntryType::Symlink => {
                let target_path = entry.link_name()?.unwrap();
                writer.append_link(&mut header, &path, target_path)?;
            }
            _ => {
                panic!("Unsupported entry type {entry_type:?}")
            }
        }
        drop(writer);

        Ok(())
    }

    pub fn finish(self) -> anyhow::Result<WrittenLayer> {
        let inner = self.archive_writer.into_inner().unwrap();
        let inner = inner.into_inner()?;
        let inner = inner.into_inner()?;
        let (_, hash) = inner.into_inner();
        Ok(WrittenLayer {
            path: self.path,
            hash,
            expected_entries: self.expected_entries,
            entries: self.entries.load(Ordering::SeqCst),
        })
    }
}

#[derive(Debug)]
pub struct WrittenLayer {
    pub path: PathBuf,
    pub hash: HashAndSize,
    pub entries: usize,
    pub expected_entries: usize,
}

impl Display for WrittenLayer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WrittenLayer: entries={:<5} expected={:<5} path={}",
            self.entries,
            self.expected_entries,
            self.path.display()
        )
    }
}
