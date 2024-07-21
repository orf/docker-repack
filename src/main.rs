use crate::image_parser::{
    Image, ImageWriter, ItemHistogram, LayerContents, LayerPacker, TarItem
};
use anyhow::Context;
use clap::Parser;
use image_parser::path_filter::PathFilter;
use indicatif::MultiProgress;
use itertools::Itertools;
use rayon::prelude::*;
use std::io::Read;
use std::path::PathBuf;

mod image_parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    root_dir: PathBuf,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let mut image = Image::from_dir(args.root_dir.join("image"))?;

    let multi_progress = MultiProgress::new();
    let all_operations: Result<Vec<(_, _)>, anyhow::Error> = image
        .layers()
        .into_par_iter()
        .map(|layer| {
            let mut archive = layer.get_progress_reader(Some(&multi_progress))?;
            let items = archive
                .entries()
                .unwrap()
                .flatten()
                .map(|entry| TarItem::from_entry(layer.id, &entry).unwrap());

            Ok((
                layer.id,
                image_parser::layer_operation::LayerOperations::from_tar_items(items)?,
            ))
        })
        .collect();

    let mut layer_contents = LayerContents::default();
    for (layer_idx, operations) in all_operations? {
        image.set_layer_file_count(layer_idx, operations.regular_file_count());
        layer_contents = layer_contents.merge_operations(operations);
    }

    println!("Total items in layers: {}", layer_contents.len());

    let compression_filter = PathFilter::from_iter(layer_contents.present_paths.iter().filter_map(
        |(path, item)| {
            if item.should_attempt_compression() {
                Some((item.layer_id, path.as_str()))
            } else {
                None
            }
        },
    ));

    // let compressed_items: Result<Vec<_>, _> = image
    //     .layers()
    //     .into_par_iter()
    //     .map(|layer| {
    //         let mut archive = layer.get_progress_reader(Some(&multi_progress))?;
    //         let mut compressor = ZstdCompressor::new();
    //         let mut compressed_items = Vec::with_capacity(layer.regular_file_count);
    //         for entry in archive.entries()? {
    //             let mut entry = entry?;
    //             let item = TarItem::from_entry(layer.id, &entry)?;
    //             let str_path = item.path.to_str().unwrap();
    //             if !compression_filter.contains_path(layer.id, str_path) {
    //                 continue;
    //             }
    //             let compressed_item = compressor
    //                 .compress(item, &mut entry)
    //                 .context("Error compressing item")?;
    //             compressed_items.push(compressed_item);
    //         }
    //         Ok::<_, anyhow::Error>(compressed_items)
    //     })
    //     .collect();

    // let compressed_items = compressed_items?.into_iter().flatten().collect_vec();

    let compressed_size_histogram = ItemHistogram::size_histogram(
        compressed_items.iter().map(|c| c.compressed_size).sorted(),
        true,
    );
    let raw_size_histogram = ItemHistogram::size_histogram(
        compressed_items
            .iter()
            .map(|c| c.tar_item.raw_size)
            .sorted(),
        true,
    );
    let compression_ratio_histogram = ItemHistogram::size_histogram(
        compressed_items
            .iter()
            .map(|c| c.compression_ratio)
            .sorted(),
        false,
    );

    println!("Compressed histogram:\n{compressed_size_histogram}");
    println!("Raw Size histogram:\n{raw_size_histogram}");
    println!("Compression ratio histogram:\n{compression_ratio_histogram:#}");

    let new_image_dir = args.root_dir.join("image_new");
    std::fs::create_dir_all(&new_image_dir)?;
    let mut image_writer = ImageWriter::new(new_image_dir);

    let mut packer = LayerPacker::default();
    packer.pack_tiny_items(
        layer_contents
            .present_paths
            .values()
            .filter(|v| v.is_tiny()),
    );
    // packer.pack_compressible_files(1024 * 1024 * 25, &compressed_items);
    println!("{packer}");

    packer.create_layers(&mut image_writer)?;

    image.layers().into_par_iter().try_for_each(|layer| {
        let mut archive = layer.get_progress_reader(Some(&multi_progress))?;
        let mut buffer = vec![];
        for entry in archive.entries()? {
            let mut entry = entry?;
            let item = TarItem::from_entry(layer.id, &entry)?;
            buffer.clear();
            entry.read_to_end(&mut buffer)?;
            image_writer.add_item(item, entry, &buffer)?;
        }
        Ok::<_, anyhow::Error>(())
    })?;

    let finished = image_writer.finish()?;
    // println!("{finished:#?}");

    Ok(())
}

// mod compact_layer;
// mod compression;
// mod image_writer;
// mod index;
// mod layer_writer;
// mod writer_utils;

// use crate::compact_layer::CompactLayer;
// use crate::image_writer::ImageWriter;
// use crate::index::LayerIndexItem;
// use byte_unit::Byte;
// use clap::Parser;
// use indicatif::{ProgressBar, ProgressStyle};
// use itertools::Itertools;
// use rayon::prelude::{ParallelBridge, ParallelIterator};
// use std::fs::File;
// use std::io::{BufReader, Read};
// use std::path::PathBuf;
// use std::sync::mpsc::channel;
// use tar::Archive;

// #[derive(Parser, Debug)]
// #[command(version, about, long_about = None)]
// struct Args {
//     rootfs: PathBuf,
//     output_dir: PathBuf,
//     #[arg(short, long)]
//     image_config: PathBuf,
//     #[arg(short, long)]
//     layers: usize,
// }

//
//
// fn main2() -> anyhow::Result<()> {
//     let args = Args::parse();
//
//     std::fs::create_dir_all(&args.output_dir)?;
//
//     let file = File::open(&args.rootfs)?;
//
//     let pb = ProgressBar::new(file.metadata().unwrap().len()).with_style(
//         ProgressStyle::with_template("{wide_bar} {binary_bytes}/{binary_total_bytes}").unwrap(),
//     );
//     let file = pb.wrap_read(file);
//     let mut archive = Archive::new(BufReader::new(file));
//
//     let (tx, rx) = channel();
//
//     let thread = std::thread::spawn(move || {
//         for mut entry in archive.entries().unwrap().flatten() {
//             let mut content = vec![];
//             entry.read_to_end(&mut content).unwrap();
//             let path = entry.path().unwrap().to_path_buf();
//             let entry_type = entry.header().entry_type();
//
//             tx.send((path, entry_type, content)).unwrap();
//         }
//     });
//
//     let items: Result<Vec<_>, _> = rx
//         .into_iter()
//         .par_bridge()
//         .map_init(Vec::new, |buffer, (path, entry_type, content)| {
//             LayerIndexItem::from_header(path, entry_type, content, buffer)
//         })
//         .collect();
//     let items = items?;
//
//     thread.join().unwrap();
//
//     let files: Vec<_> = items
//         .into_iter()
//         .filter(|item| item.is_not_dir())
//         .sorted_by_key(|f| f.compressed_size)
//         .collect();
//     let total_compressed_size = Byte::from(
//         files
//             .iter()
//             .map(|f| f.compressed_size.as_u64())
//             .sum::<u64>(),
//     );
//     let largest_file: Byte = files
//         .iter()
//         .map(|f| f.compressed_size)
//         .max()
//         .unwrap_or_default();
//     let mut target_size = total_compressed_size.divide(args.layers).unwrap();
//     if target_size < largest_file {
//         target_size = largest_file;
//     }
//
//     let mut computed_layers = vec![];
//     let mut current_layer = vec![];
//     let mut total_size = Byte::from(0u64);
//     for item in files.into_iter() {
//         if total_size.add(item.compressed_size).unwrap() > target_size {
//             computed_layers.push(CompactLayer::from_vec(current_layer));
//             total_size = item.size;
//             current_layer = vec![item];
//         } else {
//             total_size = total_size.add(item.compressed_size).unwrap();
//             current_layer.push(item);
//         }
//     }
//     if !current_layer.is_empty() {
//         computed_layers.push(CompactLayer::from_vec(current_layer));
//     }
//
//     println!(
//         "Target size: {:#.1}",
//         target_size.get_adjusted_unit(byte_unit::Unit::MB)
//     );
//     println!(
//         "Largest file: {:#.1}",
//         largest_file.get_adjusted_unit(byte_unit::Unit::MB)
//     );
//     println!("Layers:");
//     for layer in computed_layers.iter() {
//         println!("- {}", layer);
//     }
//
//     let file = File::open(args.rootfs)?;
//
//     let pb = ProgressBar::new(file.metadata().unwrap().len()).with_style(
//         ProgressStyle::with_template("{wide_bar} {binary_bytes}/{binary_total_bytes}").unwrap(),
//     );
//     let file = pb.wrap_read(file);
//     let mut archive = Archive::new(BufReader::new(file));
//
//     let mut writers = computed_layers
//         .into_iter()
//         .enumerate()
//         .map(|(idx, layer)| {
//             let path = args.output_dir.join(format!("layer-{}.tar", idx));
//             layer_writer::LayerWriter::create(path, layer)
//         })
//         .collect::<Vec<_>>();
//
//     writers.insert(
//         0,
//         layer_writer::LayerWriter::create_directory_layer(
//             args.output_dir.join("layer-directories.tar"),
//         ),
//     );
//
//     println!("Writers:");
//     for writer in &writers {
//         println!("- {writer}");
//     }
//
//     let mut buffer = vec![];
//     for entry in archive.entries().unwrap() {
//         let mut entry = entry.unwrap();
//         entry.read_to_end(&mut buffer).unwrap();
//         let path = entry.path().unwrap();
//
//         let link_name = entry.link_name().unwrap();
//         let entry_type = entry.header().entry_type();
//         for writer in &mut writers {
//             if writer.should_add_entry(entry_type, &path, &link_name) {
//                 writer.add_entry(&path, &entry, &buffer);
//             }
//         }
//         buffer.clear();
//     }
//
//     for (idx, writer) in writers.iter().enumerate() {
//         println!(
//             "{idx} - written {} / paths: {}",
//             writer.written,
//             writer.paths.len()
//         );
//     }
//
//     let finished_layers: Result<Vec<_>, _> = writers.into_iter().map(|w| w.finish()).collect();
//     let finished_layers = finished_layers?;
//
//     let blobs_dir = args.output_dir.join("blobs").join("sha256");
//     std::fs::create_dir_all(&blobs_dir)?;
//
//     for written_layer in &finished_layers {
//         let hash = &written_layer.compressed.unprefixed_hash();
//         println!(
//             "{} - {}",
//             written_layer.path.display(),
//             // written_layer.layer,
//             hash
//         );
//         std::fs::rename(&written_layer.path, blobs_dir.join(hash))?;
//     }
//
//     let image_writer = ImageWriter::new(finished_layers);
//
//     let config = image_writer.create_image_config(&args.image_config)?;
//     let written_config = image_writer::write_blob(&blobs_dir, config)?;
//     let manifest = image_writer.create_manifest(written_config)?;
//     let written_manifest = image_writer::write_blob(&blobs_dir, manifest)?;
//     let index = image_writer.create_index(written_manifest)?;
//     image_writer::write_json(&args.output_dir.join("index.json"), index)?;
//
//     std::fs::write(
//         args.output_dir.join("oci-layout"),
//         "{\"imageLayoutVersion\":\"1.0.0\"}",
//     )?;
//
//     // println!("Manifest: {manifest:?}");
//     // let index = image_writer.create_index()?;
//     // println!("{index:?}");
//
//     Ok(())
// }
