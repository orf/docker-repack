use crate::image_parser::{ImageReader, ImageWriter, LayerContents, LayerPacker, TarItem};
use anyhow::Context;
use byte_unit::{Byte, UnitType};
use clap::Parser;
use indicatif::MultiProgress;
use itertools::{Either, Itertools};
use rayon::prelude::*;
use std::io::Read;
use std::path::PathBuf;

mod image_parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    root_dir: PathBuf,
    #[arg(short, long)]
    target_size: Byte,
    #[arg(short, long)]
    exclude: Option<Vec<globset::Glob>>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let glob_set = match args.exclude {
        None => None,
        Some(globs) => {
            let mut builder = globset::GlobSetBuilder::new();
            for glob in globs {
                builder.add(glob);
            }
            Some(builder.build()?)
        }
    };

    let image =
        ImageReader::from_dir(args.root_dir.join("image")).context("Error opening image")?;

    let multi_progress = MultiProgress::new();
    let all_operations: Result<Vec<_>, anyhow::Error> = image
        .layers()
        .into_par_iter()
        .map(|layer| {
            let mut archive = layer.get_progress_reader(Some(&multi_progress))?;
            let items = archive
                .entries()
                .unwrap()
                .flatten()
                .map(|mut entry| TarItem::from_entry(layer.id, &mut entry).unwrap());

            image_parser::layer_operation::LayerOperations::from_tar_items(items)
        })
        .collect();

    let mut layer_contents = LayerContents::default();
    for operations in all_operations? {
        layer_contents = layer_contents.merge_operations(operations);
    }

    if let Some(glob_set) = glob_set {
        let (excluded_count, excluded_size) = layer_contents.exclude_globs(glob_set);
        println!("Excluded {} items ({:#.1})", excluded_count, excluded_size.get_appropriate_unit(UnitType::Decimal));
    }

    println!("Total items in layers: {}", layer_contents.len());

    let total_non_empty_files = layer_contents
        .present_paths
        .values()
        .filter(|v| v.is_non_empty_file())
        .count();
    let total_unique_non_empty_files = layer_contents
        .present_paths
        .values()
        .filter_map(|v| v.content_hash())
        .unique()
        .count();
    println!("Total non-empty files: {}", total_non_empty_files);
    println!(
        "Total unique non-empty items: {}",
        total_unique_non_empty_files
    );

    let path_map = layer_contents.into_inner();

    let new_image_dir = args.root_dir.join("image_new");
    std::fs::create_dir_all(&new_image_dir)?;
    let mut image_writer = ImageWriter::new(new_image_dir)?;

    let (tiny_items, non_tiny_items): (Vec<_>, Vec<_>) = path_map.values().partition_map(|item| {
        if item.is_tiny_file() || item.is_symlink() || item.is_dir() {
            Either::Left((item.layer_id, item.path.to_str().unwrap()))
        } else {
            Either::Right(item)
        }
    });

    image_writer.add_layer("tiny-items", tiny_items.into_iter())?;

    let mut packer = LayerPacker::new("items", args.target_size.as_u64());
    packer.add_items(non_tiny_items.into_iter())?;
    println!("{packer}");
    packer.create_layers(&mut image_writer)?;

    image.layers().into_par_iter().try_for_each(|layer| {
        let mut archive = layer.get_progress_reader(Some(&multi_progress))?;
        let mut buffer = vec![];
        for entry in archive.entries()? {
            let mut entry = entry?;
            buffer.clear();
            entry.read_to_end(&mut buffer)?;
            image_writer.add_entry(layer.id, entry, &buffer)?;
        }
        Ok::<_, anyhow::Error>(())
    })?;

    let finished = image_writer.finish(image)?;
    for item in finished {
        println!("{item}");
    }

    Ok(())
}
