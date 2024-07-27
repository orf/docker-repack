use crate::image_parser::{
    ImageReader, ImageWriter, LayerContents, LayerPacker, LayerType, TarItem,
};
use anyhow::Context;
use byte_unit::{Byte, UnitType};
use clap::{Parser, Subcommand};
use clap_num::number_range;
use comfy_table::Table;
use file_mode::User;
use globset::GlobSet;
use indicatif::MultiProgress;
use itertools::{Either, Itertools};
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use zstd::zstd_safe::CompressionLevel;
mod file_combiner;
mod image_parser;

fn parse_compression_level(s: &str) -> Result<CompressionLevel, String> {
    let range = zstd::compression_level_range();
    number_range(s, *range.start(), *range.end())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    root_dir: PathBuf,
    #[arg(short, long)]
    exclude: Option<Vec<globset::Glob>>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Repack {
        #[arg(short, long)]
        target_size: Byte,
        #[arg(short, long)]
        split_file_threshold: Option<Byte>,
        #[arg(short, long, value_parser = parse_compression_level, default_value = "7")]
        compression: CompressionLevel,
    },
    LargestFiles {
        #[arg(short, long, default_value = "10")]
        limit: usize,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let exclude = create_glob_set(args.exclude)?;
    let image =
        ImageReader::from_dir(args.root_dir.join("image")).context("Error opening image")?;
    let new_image_dir = args.root_dir.join("image_new");
    std::fs::create_dir_all(&new_image_dir)?;

    let progress = MultiProgress::new();
    match args.command {
        Command::Repack {
            target_size,
            split_file_threshold,
            compression,
        } => repack(
            progress,
            image,
            new_image_dir,
            target_size,
            split_file_threshold,
            exclude,
            compression,
        ),
        Command::LargestFiles { limit } => largest_files(progress, image, exclude, limit),
    }
}

fn largest_files(
    progress: MultiProgress,
    image: ImageReader,
    exclude: Option<GlobSet>,
    limit: usize,
) -> anyhow::Result<()> {
    let layer_contents = get_layer_contents(&progress, &image, exclude)?;
    let sorted_by_size = layer_contents
        .into_inner()
        .into_values()
        .sorted_by_key(|item| item.size)
        .rev()
        .take(limit);
    let mut table = Table::new();
    table
        .set_header(["Path", "Size"])
        .add_rows(sorted_by_size.map(|item| {
            [
                format!("{}", item.path.display()),
                format!(
                    "{:#.1}",
                    Byte::from(item.size).get_appropriate_unit(UnitType::Decimal)
                ),
            ]
        }));
    println!("{table}");
    Ok(())
}

fn repack(
    progress: MultiProgress,
    image: ImageReader,
    output_dir: PathBuf,
    target_size: Byte,
    split_file_threshold: Option<Byte>,
    exclude: Option<GlobSet>,
    compression_level: CompressionLevel,
) -> anyhow::Result<()> {
    let mut image_writer = ImageWriter::new(output_dir)?;

    let layer_contents = get_layer_contents(&progress, &image, exclude)?;
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

    let (tiny_items, non_tiny_items): (Vec<_>, Vec<_>) = path_map.values().partition_map(|item| {
        if item.is_tiny_file() || item.is_symlink() || item.is_dir() {
            Either::Left((
                item.layer_id,
                item.path.to_str().unwrap(),
                0..item.size,
                None,
            ))
        } else {
            Either::Right(item)
        }
    });

    image_writer.add_layer_paths("tiny-items", tiny_items.into_iter(), LayerType::TinyItems)?;

    let mut packer = LayerPacker::new("items", target_size.as_u64());

    let (non_tiny_items, huge_items): (Vec<_>, Vec<_>) =
        non_tiny_items
            .into_iter()
            .partition_map(|item| match split_file_threshold {
                Some(split_file_threshold) if item.size >= split_file_threshold.as_u64() => {
                    Either::Right(item)
                }
                _ => Either::Left(item),
            });
    packer.add_items(non_tiny_items.into_iter())?;

    let chunked_files = if let Some(split_file_threshold) = split_file_threshold {
        huge_items
            .into_iter()
            .map(|item| (item, item.split_into_chunks(split_file_threshold.as_u64())))
            .collect_vec()
    } else {
        vec![]
    };

    packer.add_chunked_items(chunked_files.iter().flat_map(|i| i.1.iter()))?;

    println!("{packer}");
    packer.create_layers(&mut image_writer)?;

    if !chunked_files.is_empty() {
        let (_, layer) = image_writer.create_new_layer("chunked-file-index", LayerType::Files)?;
        let script = file_combiner::generate_combining_script(&chunked_files)?;
        let index = file_combiner::generate_combining_index(&chunked_files)?;
        let repack_dir = Path::new(".docker-repack/");
        layer.write_new_directory(repack_dir)?;
        layer.write_new_file_with_data(
            &repack_dir.join("combine-files.sh"),
            file_mode::Mode::empty()
                .with_protection(User::Owner, &file_mode::Protection::all_set()),
            script.as_bytes(),
        )?;
        layer.write_new_file_with_data(
            &repack_dir.join("index.txt"),
            file_mode::Mode::empty()
                .with_protection(User::Owner, &file_mode::Protection::all_set()),
            index.as_bytes(),
        )?;
    }

    for layer in image.layers().iter() {
        let mut archive = layer.get_progress_reader(Some(&progress))?;
        for entry in archive.entries()? {
            let mut entry = entry?;
            image_writer.add_entry(
                layer.id,
                &mut entry,
                split_file_threshold.map(|f| f.as_u64()),
            )?;
        }
    }
    // image.layers().into_par_iter().try_for_each(|layer| {
    //     let mut archive = layer.get_progress_reader(Some(&progress))?;
    //     for entry in archive.entries()? {
    //         let mut entry = entry?;
    //         image_writer.add_entry(
    //             layer.id,
    //             &mut entry,
    //             split_file_threshold.map(|f| f.as_u64()),
    //         )?;
    //     }
    //     Ok::<_, anyhow::Error>(())
    // })?;

    let finished_layers = image_writer.finish_writing_layers()?;
    let compressed_layers =
        image_writer.compress_layers(&progress, finished_layers, compression_level)?;
    let sorted_layers = compressed_layers
        .into_iter()
        .sorted_by_key(|(layer, size)| (layer.type_, size.size))
        .collect_vec();
    image_writer.write_index(&sorted_layers, image)?;
    let total_size = sorted_layers.iter().map(|(_, size)| size.size).sum::<u64>();
    for (layer, hash_and_size) in sorted_layers {
        println!(
            "{layer} - compressed: {} / Size: {:#.1}",
            hash_and_size.raw_hash(),
            Byte::from(hash_and_size.size).get_appropriate_unit(UnitType::Decimal)
        );
    }
    println!(
        "Total image size: {:#.1}",
        Byte::from(total_size).get_appropriate_unit(UnitType::Decimal)
    );

    Ok(())
}

fn get_layer_contents(
    progress: &MultiProgress,
    image: &ImageReader,
    exclude: Option<GlobSet>,
) -> anyhow::Result<LayerContents> {
    let all_operations: Result<Vec<_>, anyhow::Error> = image
        .layers()
        .into_par_iter()
        .map(|layer| {
            let mut archive = layer.get_progress_reader(Some(progress))?;
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

    if let Some(glob_set) = exclude {
        let (excluded_count, excluded_size) = layer_contents.exclude_globs(glob_set);
        println!(
            "Excluded {} items ({:#.1})",
            excluded_count,
            excluded_size.get_appropriate_unit(UnitType::Decimal)
        );
    }
    Ok(layer_contents)
}

fn create_glob_set(exclude: Option<Vec<globset::Glob>>) -> anyhow::Result<Option<GlobSet>> {
    let glob_set = match exclude {
        None => None,
        Some(globs) => {
            let mut builder = globset::GlobSetBuilder::new();
            for glob in globs {
                builder.add(glob);
            }
            Some(builder.build()?)
        }
    };

    Ok(glob_set)
}
