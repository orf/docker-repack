use crate::io::image::reader::SourceLayerID;
use crate::io::image::writer::ImageWriter;
use crate::io::image::writer::NewLayerID;
use crate::io::layer::reader::DecompressedLayer;
use crate::tar_item::TarItem;
use crate::utils::create_pbar;
use indicatif::{MultiProgress, ProgressIterator};
use itertools::Itertools;
use memmap2::Mmap;
use std::io::Cursor;
use std::ops::Range;
use std::path::PathBuf;
use tar::Archive;
use tracing::{info, trace};

#[derive(Debug, strum_macros::Display)]
pub enum RepackOperationType {
    WriteWholeItem,
    WritePartialItem(Range<u64>, PathBuf),
}

pub type SortKey = String;

#[derive(Debug)]
pub struct RepackOperation {
    source: SourceLayerID,
    item_offset: u64,

    dest: NewLayerID,
    sort_key: SortKey,
    type_: RepackOperationType,
}

impl RepackOperation {
    pub fn new_whole_item(item: &TarItem, dest: NewLayerID) -> Self {
        RepackOperation {
            source: item.layer_id,
            item_offset: item.header_position,
            dest,
            sort_key: Self::sort_key(item),
            type_: RepackOperationType::WriteWholeItem,
        }
    }

    pub fn new_partial_item(
        item: &TarItem,
        dest: NewLayerID,
        range: Range<u64>,
        path: PathBuf,
    ) -> Self {
        RepackOperation {
            source: item.layer_id,
            item_offset: item.header_position,
            dest,
            sort_key: Self::sort_key(item),
            type_: RepackOperationType::WritePartialItem(range, path),
        }
    }
    fn sort_key(item: &TarItem) -> SortKey {
        item.path.to_str().unwrap().to_string()
    }
}

#[derive(Debug)]
pub struct RepackPlan {
    operations: Vec<RepackOperation>,
}

impl RepackPlan {
    pub fn new(capacity: usize) -> Self {
        Self {
            operations: Vec::with_capacity(capacity),
        }
    }
    pub fn add_full_item(&mut self, dest: NewLayerID, item: &TarItem) {
        self.operations
            .push(RepackOperation::new_whole_item(item, dest));
    }

    pub fn add_partial_item(
        &mut self,
        dest: NewLayerID,
        item: &TarItem,
        range: Range<u64>,
        path: PathBuf,
    ) {
        self.operations
            .push(RepackOperation::new_partial_item(item, dest, range, path));
    }

    pub fn summarize(&self) -> PlanStats {
        let dest_layer_count = self.operations.iter().map(|v| v.dest).unique().count();
        let source_layer_count = self.operations.iter().map(|v| v.source).unique().count();
        PlanStats {
            total_operations: self.operations.len(),
            source_layer_count,
            dest_layer_count,
        }
    }

    pub fn execute(
        mut self,
        progress: &MultiProgress,
        mut image_writer: ImageWriter,
        source_layers: Vec<DecompressedLayer>,
    ) -> anyhow::Result<ImageWriter> {
        info!("Executing plan");
        self.operations.sort_by(|r1, r2| {
            (r1.dest, &r1.sort_key, r1.source).cmp(&(r2.dest, &r2.sort_key, r2.source))
        });

        let progress_bar = create_pbar(progress, self.operations.len() as u64, "Repacking", false);

        for (source_layer_id, chunk) in &self
            .operations
            .into_iter()
            .progress_with(progress_bar)
            .chunk_by(|r| r.source)
        {
            let source_layer_data = source_layers[source_layer_id.0].get_raw_mmap()?;
            let mut source_layer_archive = SeekableArchive::new(&source_layer_data, 0);

            let chunk = chunk.collect_vec();
            let progress = create_pbar(
                progress,
                chunk.len() as u64,
                format!("Copying files from layer {source_layer_id}"),
                false,
            );

            for (new_layer_id, chunk) in &chunk
                .into_iter()
                .progress_with(progress)
                .chunk_by(|r| r.dest)
            {
                let new_layer_writer = image_writer.get_layer(new_layer_id);
                for operation in chunk {
                    trace!(
                        "path={} sort_key={:?}",
                        operation.item_offset,
                        operation.sort_key
                    );
                    source_layer_archive =
                        source_layer_archive.seek_to(operation.item_offset as usize)?;
                    match operation.type_ {
                        RepackOperationType::WriteWholeItem => {
                            let item = source_layer_archive.read_entry()?;
                            new_layer_writer.copy_item(item)?;
                        }
                        RepackOperationType::WritePartialItem(range, new_path) => {
                            let item = source_layer_archive.read_entry()?;
                            let complete_item_data =
                                &source_layer_data[item.raw_file_position() as usize..];
                            let data =
                                &complete_item_data[(range.start as usize)..(range.end as usize)];
                            assert_eq!(data.len() as u64, range.end - range.start);
                            new_layer_writer.copy_partial_item(item, range, new_path, data)?;
                        }
                    }
                }
            }
        }
        info!("Plan executed");
        Ok(image_writer)
    }
}

pub struct PlanStats {
    pub total_operations: usize,
    pub source_layer_count: usize,
    pub dest_layer_count: usize,
}

pub struct SeekableArchive<'a> {
    mmap: &'a Mmap,
    archive: Archive<Cursor<&'a [u8]>>,
}

impl<'a> SeekableArchive<'a> {
    pub fn new(mmap: &'a Mmap, index: usize) -> Self {
        let memory_slice = &mmap[index..];
        let archive = Archive::new(Cursor::new(memory_slice));
        Self { mmap, archive }
    }

    pub fn seek_to(self, location: usize) -> anyhow::Result<Self> {
        Ok(Self::new(self.mmap, location))
    }

    pub fn read_entry(&mut self) -> anyhow::Result<tar::Entry<Cursor<&'a [u8]>>> {
        let mut entries = self.archive.entries()?;
        let next = entries.next().unwrap()?;
        Ok(next)
    }
}
