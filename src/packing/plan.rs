use crate::io::image::reader::SourceLayerID;
use crate::io::image::writer::ImageWriter;
use crate::io::image::writer::NewLayerID;
use crate::io::layer::reader::DecompressedLayer;
use crate::tar_item::{TarItem, TarItemSortKey};
use crate::utils::create_pbar;
use indicatif::{MultiProgress, ProgressIterator};
use itertools::Itertools;
use tracing::{info, trace};

#[cfg(feature = "split_files")]
use std::ops::Range;
#[cfg(feature = "split_files")]
use std::path::PathBuf;

#[derive(Debug, strum_macros::Display)]
pub enum RepackOperationType {
    WriteWholeItem,
    #[cfg(feature = "split_files")]
    WritePartialItem(Range<u64>, PathBuf),
}

#[derive(Debug)]
pub struct RepackOperation {
    source: SourceLayerID,
    item_offset: u64,

    dest: NewLayerID,
    sort_key: TarItemSortKey,
    type_: RepackOperationType,
}

impl RepackOperation {
    pub fn new_whole_item(item: &TarItem, dest: NewLayerID) -> Self {
        RepackOperation {
            source: item.layer_id,
            item_offset: item.header_position,
            dest,
            sort_key: item.sort_key(),
            type_: RepackOperationType::WriteWholeItem,
        }
    }

    #[cfg(feature = "split_files")]
    pub fn new_partial_item(item: &TarItem, dest: NewLayerID, range: Range<u64>, path: PathBuf) -> Self {
        RepackOperation {
            source: item.layer_id,
            item_offset: item.header_position,
            dest,
            sort_key: item.sort_key(),
            type_: RepackOperationType::WritePartialItem(range, path),
        }
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
        self.operations.push(RepackOperation::new_whole_item(item, dest));
    }

    #[cfg(feature = "split_files")]
    pub fn add_partial_item(&mut self, dest: NewLayerID, item: &TarItem, range: Range<u64>, path: PathBuf) {
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
        self.operations
            .sort_by(|r1, r2| (r1.dest, &r1.sort_key, r1.source).cmp(&(r2.dest, &r2.sort_key, r2.source)));

        let progress_bar = create_pbar(progress, self.operations.len() as u64, "Repacking", false);

        let seekable_readers: Result<Vec<_>, _> = source_layers.iter().map(|r| r.get_seekable_reader()).collect();
        let seekable_readers = seekable_readers?;

        let mut total_done = 0;
        let total = self.operations.len();
        let ten_percent = self.operations.len() / 10;

        let is_pbar_hidden = progress.is_hidden();

        for (source_layer_id, chunk) in &self
            .operations
            .into_iter()
            .progress_with(progress_bar)
            .chunk_by(|r| r.source)
        {
            let seekable_reader = &seekable_readers[source_layer_id.0];
            let chunk = chunk.collect_vec();

            for (new_layer_id, chunk) in &chunk.into_iter().chunk_by(|r| r.dest) {
                let new_layer_writer = image_writer.get_layer(new_layer_id);
                for operation in chunk {
                    trace!("path={} sort_key={:?}", operation.item_offset, operation.sort_key);
                    let mut item_archive = seekable_reader.open_position(operation.item_offset as usize);

                    total_done += 1;

                    if is_pbar_hidden && (total_done % ten_percent) == 0 {
                        info!("Written {total_done}/{total}");
                    }

                    match operation.type_ {
                        RepackOperationType::WriteWholeItem => {
                            let item = item_archive.read_entry()?;
                            new_layer_writer.copy_item(item)?;
                        }
                        #[cfg(feature = "split_files")]
                        RepackOperationType::WritePartialItem(range, new_path) => {
                            todo!();
                            // let item = item_archive.read_entry()?;
                            // let complete_item_data =
                            // &source_layer_data[item.raw_file_position() as usize..];
                            // let data =
                            //     &complete_item_data[(range.start as usize)..(range.end as usize)];
                            // assert_eq!(data.len() as u64, range.end - range.start);
                            // new_layer_writer.copy_partial_item(item, range, new_path, data)?;
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
