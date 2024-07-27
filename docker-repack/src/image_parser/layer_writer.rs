use crate::image_parser::image_writer::NewLayerID;
use crate::image_parser::{HashAndSize, HashedWriter};
use byte_unit::{Byte, UnitType};
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io::Write;
use std::io::{BufWriter, Read};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use tar::{Builder, EntryType, Header};

#[derive(Debug, Ord, PartialOrd, Eq, PartialEq, Copy, Clone)]
pub enum LayerType {
    TinyItems,
    Files,
}

pub struct LayerWriter {
    id: NewLayerID,
    path: PathBuf,
    archive_writer: Mutex<Builder<BufWriter<HashedWriter<File>>>>,
    index_writer: Mutex<BufWriter<File>>,
    entries: AtomicUsize,
    type_: LayerType,
}

impl LayerWriter {
    pub fn create_layer(
        id: NewLayerID,
        path: PathBuf,
        type_: LayerType,
    ) -> anyhow::Result<LayerWriter> {
        let writer = HashedWriter::new(File::create(&path)?);
        let writer = Builder::new(BufWriter::new(writer));
        let index_writer = BufWriter::new(File::create(path.with_extension("index.txt"))?);
        Ok(LayerWriter {
            id,
            path,
            archive_writer: Mutex::new(writer),
            index_writer: Mutex::new(index_writer),
            entries: 0.into(),
            type_,
        })
    }

    #[inline(always)]
    fn write_index(
        &self,
        byte_range: &Range<u64>,
        path: &Path,
        link_name: Option<&Path>,
        entry_type: EntryType,
    ) -> anyhow::Result<()> {
        let prev = self.entries.fetch_add(1, Ordering::Relaxed);
        let mut index_writer = self.index_writer.lock().unwrap();
        writeln!(
            index_writer,
            "{prev:>10} {entry_type:>20?} - byte_range: {:>10?} path: {} link: {:?}",
            byte_range,
            path.display(),
            link_name
        )?;
        drop(index_writer);
        Ok(())
    }

    #[inline(always)]
    pub fn write_new_directory(&self, path: &Path) -> anyhow::Result<()> {
        self.write_index(&(0..0), path, None, EntryType::Directory)?;
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Directory);
        header.set_size(0);
        let mut writer = self.archive_writer.lock().unwrap();
        writer.append_data(&mut header, path, &mut std::io::empty())?;
        Ok(())
    }

    #[inline(always)]
    pub fn write_new_file_with_data(
        &self,
        path: &Path,
        file_mode: file_mode::Mode,
        data: &[u8],
    ) -> anyhow::Result<()> {
        self.write_index(&(0..data.len() as u64), path, None, EntryType::Regular)?;
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_size(data.len() as u64);
        header.set_mode(file_mode.mode());
        let mut writer = self.archive_writer.lock().unwrap();
        writer.append_data(&mut header, path, data)?;
        Ok(())
    }

    #[inline(always)]
    pub fn write_file(
        &self,
        mut header: Header,
        path: &Path,
        mut data: impl Read,
        byte_range: Range<u64>,
    ) -> anyhow::Result<()> {
        self.write_index(&byte_range, path, None, header.entry_type())?;
        let mut writer = self.archive_writer.lock().unwrap();
        writer.append_data(&mut header, path, &mut data)?;
        Ok(())
    }

    #[inline(always)]
    pub fn write_empty_file(&self, mut header: Header, path: &Path) -> anyhow::Result<()> {
        self.write_index(&(0..0), path, None, header.entry_type())?;
        let mut writer = self.archive_writer.lock().unwrap();
        writer.append_data(&mut header, path, &mut std::io::empty())?;
        Ok(())
    }

    #[inline(always)]
    pub fn write_link(
        &self,
        mut header: Header,
        path: &Path,
        target_path: &Path,
    ) -> anyhow::Result<()> {
        self.write_index(&(0..0), path, Some(target_path), header.entry_type())?;
        let mut writer = self.archive_writer.lock().unwrap();
        writer.append_link(&mut header, path, target_path)?;
        Ok(())
    }

    #[inline(always)]
    pub fn write_directory(&self, mut header: Header, path: &Path) -> anyhow::Result<()> {
        self.write_index(&(0..0), path, None, header.entry_type())?;
        let mut writer = self.archive_writer.lock().unwrap();
        writer.append_data(&mut header, path, &mut std::io::empty())?;
        Ok(())
    }

    pub fn finish(self) -> anyhow::Result<WrittenLayer> {
        let inner = self.archive_writer.into_inner().unwrap();
        let inner = inner.into_inner()?;
        let inner = inner.into_inner()?;
        let (_, hash) = inner.into_inner();
        Ok(WrittenLayer {
            id: self.id,
            type_: self.type_,
            path: self.path,
            hash,
            entries: self.entries.load(Ordering::SeqCst),
        })
    }
}

#[derive(Debug)]
pub struct WrittenLayer {
    pub id: NewLayerID,
    pub type_: LayerType,
    pub path: PathBuf,
    pub hash: HashAndSize,
    pub entries: usize,
}

impl Display for WrittenLayer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "WrittenLayer: ")?;
        write!(
            f,
            "size={:#<6.1} entries={:<5} path={}",
            Byte::from(self.hash.size).get_appropriate_unit(UnitType::Decimal),
            self.entries,
            self.path.display()
        )
    }
}
