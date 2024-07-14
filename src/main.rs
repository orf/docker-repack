mod compact_layer;
mod image_writer;
mod index;
mod layer_writer;
mod writer_utils;
mod compression;

use crate::compact_layer::CompactLayer;
use crate::image_writer::ImageWriter;
use crate::index::LayerIndexItem;
use byte_unit::Byte;
use clap::Parser;
use indicatif::{ProgressBar, ProgressStyle};
use itertools::Itertools;
use rayon::prelude::{ParallelBridge, ParallelIterator};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use tar::Archive;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    rootfs: PathBuf,
    output_dir: PathBuf,
    #[arg(short, long)]
    image_config: PathBuf,
    #[arg(short, long)]
    layers: usize,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    std::fs::create_dir_all(&args.output_dir)?;

    let file = File::open(&args.rootfs)?;

    let pb = ProgressBar::new(file.metadata().unwrap().len()).with_style(
        ProgressStyle::with_template("{wide_bar} {binary_bytes}/{binary_total_bytes}").unwrap(),
    );
    let file = pb.wrap_read(file);
    let mut archive = Archive::new(BufReader::new(file));

    let (tx, rx) = channel();

    let thread = std::thread::spawn(move || {
        for mut entry in archive.entries().unwrap().flatten() {
            let mut content = vec![];
            entry.read_to_end(&mut content).unwrap();
            let path = entry.path().unwrap().to_path_buf();
            let entry_type = entry.header().entry_type();

            tx.send((path, entry_type, content)).unwrap();
        }
    });

    let items: Result<Vec<_>, _> = rx
        .into_iter()
        .par_bridge()
        .map_init(Vec::new, |buffer, (path, entry_type, content)| {
            LayerIndexItem::from_header(path, entry_type, content, buffer)
        })
        .collect();
    let items = items?;

    thread.join().unwrap();

    let files: Vec<_> = items
        .into_iter()
        .filter(|item| item.is_not_dir())
        .sorted_by_key(|f| f.compressed_size)
        .collect();
    let total_compressed_size = Byte::from(
        files
            .iter()
            .map(|f| f.compressed_size.as_u64())
            .sum::<u64>(),
    );
    let largest_file: Byte = files
        .iter()
        .map(|f| f.compressed_size)
        .max()
        .unwrap_or_default();
    let mut target_size = total_compressed_size.divide(args.layers).unwrap();
    if target_size < largest_file {
        target_size = largest_file;
    }

    let mut computed_layers = vec![];
    let mut current_layer = vec![];
    let mut total_size = Byte::from(0u64);
    for item in files.into_iter() {
        if total_size.add(item.compressed_size).unwrap() > target_size {
            computed_layers.push(CompactLayer::from_vec(current_layer));
            total_size = item.size;
            current_layer = vec![item];
        } else {
            total_size = total_size.add(item.compressed_size).unwrap();
            current_layer.push(item);
        }
    }
    if !current_layer.is_empty() {
        computed_layers.push(CompactLayer::from_vec(current_layer));
    }

    println!(
        "Target size: {:#.1}",
        target_size.get_adjusted_unit(byte_unit::Unit::MB)
    );
    println!(
        "Largest file: {:#.1}",
        largest_file.get_adjusted_unit(byte_unit::Unit::MB)
    );
    for layer in computed_layers.iter() {
        println!("{} - total paths: {}", layer, layer.paths.len());
    }

    let file = File::open(args.rootfs)?;

    let pb = ProgressBar::new(file.metadata().unwrap().len()).with_style(
        ProgressStyle::with_template("{wide_bar} {binary_bytes}/{binary_total_bytes}").unwrap(),
    );
    let file = pb.wrap_read(file);
    let mut archive = Archive::new(BufReader::new(file));

    let mut writers = computed_layers
        .into_iter()
        .enumerate()
        .map(|(idx, layer)| {
            let path = args.output_dir.join(format!("layer-{}.tar", idx));
            let include_symlinks = idx == 0;
            layer_writer::LayerWriter::create(path, layer, include_symlinks)
        })
        .collect::<Vec<_>>();

    let mut buffer = vec![];
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        entry.read_to_end(&mut buffer).unwrap();
        let path = entry.path().unwrap();

        let link_name = entry.link_name().unwrap();
        let entry_type = entry.header().entry_type();
        for writer in &mut writers {
            if writer.should_add_entry(entry_type, &path, &link_name) {
                writer.add_entry(&path, &entry, &buffer);
            }
        }
        buffer.clear();
    }

    for (idx, writer) in writers.iter().enumerate() {
        println!(
            "{idx} - written {} / paths: {}",
            writer.written,
            writer.layer.paths.len()
        );
    }

    let finished_layers: Result<Vec<_>, _> = writers.into_iter().map(|w| w.finish()).collect();
    let finished_layers = finished_layers?;

    let blobs_dir = args.output_dir.join("blobs").join("sha256");
    std::fs::create_dir_all(&blobs_dir)?;

    for written_layer in &finished_layers {
        let hash = &written_layer.compressed.unprefixed_hash();
        println!(
            "{} - {}",
            written_layer.path.display(),
            // written_layer.layer,
            hash
        );
        std::fs::rename(&written_layer.path, blobs_dir.join(hash))?;
    }

    let image_writer = ImageWriter::new(finished_layers);

    let config = image_writer.create_image_config(&args.image_config)?;
    let written_config = image_writer::write_blob(&blobs_dir, config)?;
    let manifest = image_writer.create_manifest(written_config)?;
    let written_manifest = image_writer::write_blob(&blobs_dir, manifest)?;
    let index = image_writer.create_index(written_manifest)?;
    image_writer::write_json(
        &args.output_dir.join("index.json"),
        &index,
    )?;

    std::fs::write(
        args.output_dir.join("oci-layout"),
        "{\"imageLayoutVersion\":\"1.0.0\"}",
    )?;

    // println!("Manifest: {manifest:?}");
    // let index = image_writer.create_index()?;
    // println!("{index:?}");

    Ok(())
}
