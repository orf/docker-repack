use crate::io::image::reader::ImageReader;
use crate::io::image::writer::ImageWriter;
use crate::packing::RepackPlan;
use crate::packing::{CompressedLayerPacker, LayerPacker};
use crate::utils::display_bytes;
use byte_unit::Byte;
use globset::GlobSet;
use indicatif::{MultiProgress, ProgressIterator};
use itertools::Itertools;
use std::path::PathBuf;

use tracing::{debug, info};
use zstd::zstd_safe::CompressionLevel;

use crate::content::merged::MergedLayerContent;
#[cfg(feature = "split_files")]
use crate::packing::FileCombiner;
use crate::utils;
#[cfg(feature = "split_files")]
use std::path::Path;

pub fn repack(
    progress: MultiProgress,
    image: ImageReader,
    output_dir: PathBuf,
    target_size: Byte,
    #[cfg(feature = "split_files")] split_files: Option<Byte>,
    exclude: Option<GlobSet>,
    compression_level: CompressionLevel,
    skip_compression: bool,
    keep_temp_files: bool,
    // repack_type: RepackType,
) -> anyhow::Result<()> {
    info!("Found {} layers", image.layers().len());
    info!("Total compressed size: {:#.1}", display_bytes(image.compressed_size()));

    let mut image_writer = ImageWriter::new(output_dir)?;

    let (decompressed_layers, image_config) = image.decompress_layers(&image_writer, &progress)?;

    let layer_contents = crate::cmd::utils::get_layer_contents(&progress, &decompressed_layers, exclude)?;
    info!("Image read complete:");

    print_layer_contents_stats(&layer_contents);

    let path_map = layer_contents.into_inner();

    let tiny_items_layer = image_writer.create_new_layer("tiny-items")?;
    let mut layer_packer = CompressedLayerPacker::new(image_writer, target_size.as_u64())?;

    let sorted_tar_items = path_map
        .values()
        .sorted_by(|v1, v2| v1.sort_key().cmp(&v2.sort_key()))
        .collect_vec();

    let decompressed_layer_readers: anyhow::Result<Vec<_>> =
        decompressed_layers.iter().map(|l| l.get_seekable_reader()).collect();
    let decompressed_layer_readers = decompressed_layer_readers?;

    let mut plan = RepackPlan::new(path_map.len());
    #[cfg(feature = "split_files")]
    let mut combiner = FileCombiner::new();

    let pbar = utils::create_pbar(&progress, sorted_tar_items.len() as u64, "Planning Repacking", false);

    for item in sorted_tar_items.iter().progress_with(pbar) {
        if item.is_tiny_file() || item.is_symlink() || item.is_dir() {
            plan.add_full_item(tiny_items_layer, item)
        } else {
            #[cfg(feature = "split_files")]
            if let Some(split_size) = split_files {
                if item.size > target_size {
                    let key = item.key();
                    let chunks = item.split_into_chunks(split_size.as_u64());
                    for chunk in &chunks {
                        let layer_id = layer_packer.layer_for(key, chunk.size(), None, None);
                        plan.add_partial_item(layer_id, item, chunk.byte_range.clone(), chunk.dest_path())
                    }
                    combiner.add_chunked_file(item, chunks);
                    continue;
                }
            }

            let decompressed_layer = &decompressed_layer_readers[item.layer_id.0];
            let data_slice = decompressed_layer.get_data_slice(item);

            let layer_id = layer_packer.layer_for_item(item, data_slice)?;
            plan.add_full_item(layer_id, item)
        }
    }

    #[cfg(feature = "split_files")]
    let mut image_writer = layer_packer.into_inner();
    #[cfg(not(feature = "split_files"))]
    let image_writer = layer_packer.into_inner();

    #[cfg(feature = "split_files")]
    let entrypoint_override = if !combiner.is_empty() {
        info!("{} files will be split into chunks", combiner.len());
        let tiny_items_layer_writer = image_writer.get_layer(tiny_items_layer);
        let base_path = Path::new(".docker-repack/");
        Some(combiner.write_to_image(base_path, tiny_items_layer_writer)?)
    } else {
        None
    };

    let summary = plan.summarize();
    info!(
        "Plan finished: creating {} new layers from {} source layers, with {} total operations",
        summary.dest_layer_count, summary.source_layer_count, summary.total_operations
    );

    let mut image_writer = plan.execute(&progress, image_writer, decompressed_layers)?;
    let finished_layers = image_writer.finish_writing_layers()?;

    let final_layers = if !skip_compression {
        image_writer.write_compressed_layers(&progress, finished_layers, compression_level, keep_temp_files)?
    } else {
        image_writer.write_uncompressed_layers(finished_layers)?
    };

    let sorted_layers = final_layers
        .into_iter()
        .enumerate()
        .sorted_by_key(|(idx, (_, size))| (*idx != 0, size.size))
        .map(|(_, item)| item)
        .collect_vec();

    if !keep_temp_files {
        image_writer.remove_temp_files()?;
    }

    image_writer.write_index(
        &sorted_layers,
        image_config,
        skip_compression,
        #[cfg(feature = "split_files")]
        entrypoint_override,
    )?;

    let total_size = sorted_layers.iter().map(|(_, size)| size.size).sum::<u64>();
    info!("Total image size  : {:#.1}", display_bytes(total_size));
    info!("Total image layers: {}", sorted_layers.len());

    for (layer, hash_and_size) in sorted_layers {
        debug!(
            "{layer} - compressed: {} / Size: {:#.1}",
            hash_and_size.raw_hash(),
            display_bytes(hash_and_size.size)
        );
    }

    Ok(())
}

fn print_layer_contents_stats(layer_contents: &MergedLayerContent) {
    info!(
        "Total items: {} ({:#.1})",
        layer_contents.added_files.count,
        display_bytes(layer_contents.added_files.size)
    );
    info!(
        "Total removed: {} ({:#.1})",
        layer_contents.removed_files.count,
        display_bytes(layer_contents.removed_files.size)
    );
    info!(
        "Total excluded: {} ({:#.1})",
        layer_contents.excluded_files.count,
        display_bytes(layer_contents.excluded_files.size)
    );

    info!("Total items in output: {}", layer_contents.len());
    let non_empty_files_count = layer_contents.non_empty_files().count();
    let unique_non_empty_files_count = layer_contents.unique_non_empty_files_count();
    info!("Non-empty file count: {}", non_empty_files_count);
    info!("Unique non-empty file count: {}", unique_non_empty_files_count);
    info!(
        "Duplicate files: {}",
        non_empty_files_count - unique_non_empty_files_count
    );
    info!(
        "Total items removed from output: {}",
        layer_contents.added_files.count - layer_contents.len() as u64
    );
    info!("Total raw size: {:#.1}", display_bytes(layer_contents.total_size()));
}
