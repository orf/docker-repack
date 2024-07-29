use crate::io::hashed_writer::{HashAndSize, HashedWriter};
use crate::io::image::writer::NewLayerID;
use crate::io::new_bufwriter;
use crate::utils::display_bytes;
use anyhow::bail;
use std::fmt::{Display, Formatter};
use std::fs::File;
use std::io;
use std::io::Write;
use std::io::{BufWriter, Read, Seek};
use std::ops::Range;
use std::path::{Path, PathBuf};
use tar::{Builder, Entry, EntryType, Header};

pub struct LayerWriter {
    id: NewLayerID,
    path: PathBuf,
    archive_writer: Builder<BufWriter<HashedWriter<File>>>,
    index_writer: BufWriter<File>,
    written_entries: u64,
}

impl LayerWriter {
    pub fn create_layer(id: NewLayerID, path: PathBuf) -> anyhow::Result<LayerWriter> {
        let writer = HashedWriter::new(File::create(&path)?);
        let writer = Builder::new(new_bufwriter(writer));
        let index_writer = new_bufwriter(File::create(path.with_extension("index.txt"))?);
        Ok(LayerWriter {
            id,
            path,
            archive_writer: writer,
            index_writer,
            written_entries: 0,
        })
    }

    fn write_index(
        &mut self,
        byte_range: &Range<u64>,
        item: &Entry<impl Read>,
    ) -> anyhow::Result<()> {
        writeln!(
            self.index_writer,
            "{:<5} {:?}\t\tsize={:#10.1} path={:<100} link={:?}",
            self.written_entries,
            item.header().entry_type(),
            display_bytes(byte_range.end - byte_range.start),
            item.path()?.display(),
            item.link_name()?.as_ref().map(|p| p.display()),
        )?;
        self.written_entries += 1;
        Ok(())
    }

    pub fn new_directory(&mut self, path: impl AsRef<Path>, mode: u32) -> anyhow::Result<()> {
        for parent in path.as_ref().components().rev() {
            let mut header = Header::new_gnu();
            header.set_entry_type(EntryType::Directory);
            header.set_mode(mode);
            self.archive_writer
                .append_data(&mut header, parent, &mut io::empty())?;
        }
        Ok(())
    }

    pub fn new_item(&mut self, path: &Path, mode: u32, data: &[u8]) -> anyhow::Result<()> {
        let mut header = Header::new_gnu();
        header.set_entry_type(EntryType::Regular);
        header.set_mode(mode);
        header.set_size(data.len() as u64);
        self.archive_writer.append_data(&mut header, path, data)?;
        Ok(())
    }

    pub fn copy_item(&mut self, item: Entry<impl Read>) -> anyhow::Result<()> {
        let byte_range = 0..item.size();
        self.write_index(&byte_range, &item)?;

        let entry_type = item.header().entry_type();
        match entry_type {
            EntryType::Regular | EntryType::Link | EntryType::Symlink | EntryType::Directory => {
                self.archive_writer.append(&item.header().clone(), item)?;
            }
            type_ => {
                bail!("Unsupported entry type: {:?}", type_);
            }
        }
        Ok(())
    }

    pub fn copy_partial_item(
        &mut self,
        item: Entry<impl Read + Seek>,
        range: Range<u64>,
        new_path: PathBuf,
        data: &[u8],
    ) -> anyhow::Result<()> {
        let byte_range = range.start..range.end;
        self.write_index(&byte_range, &item)?;

        let entry_type = item.header().entry_type();
        match entry_type {
            EntryType::Regular | EntryType::Link | EntryType::Symlink | EntryType::Directory => {
                let mut header = item.header().clone();
                header.set_size(data.len() as u64);
                self.archive_writer
                    .append_data(&mut header, new_path, data)?;
            }
            type_ => {
                bail!("Unsupported entry type: {:?}", type_);
            }
        }
        Ok(())
    }

    pub fn finish(self) -> anyhow::Result<WrittenLayer> {
        let inner = self.archive_writer.into_inner().unwrap();
        let inner = inner.into_inner()?;
        let (_, hash) = inner.into_inner();
        Ok(WrittenLayer {
            id: self.id,
            path: self.path,
            hash,
            entries: self.written_entries,
        })
    }
}

#[derive(Debug)]
pub struct WrittenLayer {
    pub id: NewLayerID,
    pub path: PathBuf,
    pub hash: HashAndSize,
    pub entries: u64,
}

impl Display for WrittenLayer {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "WrittenLayer: ")?;
        write!(
            f,
            "size={:#<6.1} entries={:<5} path={}",
            display_bytes(self.hash.size),
            self.entries,
            self.path.display()
        )
    }
}
