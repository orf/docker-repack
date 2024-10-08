use crate::index::{ImageItem, ImageItems};
use crate::input::remote_image::RemoteImage;
use crate::layer_combiner::LayerCombiner;
use anyhow::{bail, Context};
use byte_unit::Byte;
use clap::Parser;
use globset::Glob;
use input::InputImage;
use itertools::Itertools;
use memmap2::Mmap;
use oci_spec::image::Sha256Digest;
use output_image::image::OutputImageWriter;
use output_image::layers::OutputLayers;
use rand::prelude::*;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fs::File;
use std::path::Path;
use tracing::{info, info_span, instrument, Level};
use tracing_indicatif::IndicatifLayer;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod compression;
mod index;
mod input;
mod io_utils;
mod layer_combiner;
pub mod location;
mod output_image;
mod platform_matcher;
mod progress;
#[cfg(test)]
mod test_utils;

use crate::input::local_image::LocalOciImage;
use crate::platform_matcher::PlatformMatcher;
use crate::progress::{display_bytes, progress_parallel_collect};
use location::Location;
use output_image::stats::WrittenImageStats;
use shadow_rs::shadow;
use tracing_subscriber::filter::Directive;
use tracing_subscriber::EnvFilter;

shadow!(build);

#[derive(Parser, Debug)]
#[clap(version = build::CLAP_LONG_VERSION)]
struct Args {
    /// Source image. e.g. `python:3.11`, `tensorflow/tensorflow:latest` or `oci://local/image/path`
    source: Location,
    /// Location to save image, e.g oci://directory/path/
    output_dir: Location,
    /// Target size for layers
    #[arg(long, short)]
    target_size: Byte,

    #[arg(long)]
    concurrency: Option<usize>,

    #[arg(long)]
    keep_temp_files: bool,

    #[arg(long, default_value = "14")]
    compression_level: i32,

    #[arg(long, default_value = "linux/*")]
    platform: Glob,
}

pub fn main() -> anyhow::Result<()> {
    let indicatif_layer = IndicatifLayer::new().with_max_progress_bars(14, None);
    let env_builder = EnvFilter::builder()
        .with_default_directive(Directive::from(Level::INFO))
        .from_env()?;
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .compact()
                .with_thread_names(true)
                .with_writer(indicatif_layer.get_stderr_writer()),
        )
        .with(indicatif_layer)
        .with(env_builder)
        .init();
    let args = Args::parse();

    let output_dir = match args.output_dir {
        Location::Oci(path) => path,
        Location::Docker(_) => {
            bail!("Docker registry output is not currently supported")
        }
    };

    let temp_dir = output_dir.join("temp");
    let target_size = args.target_size;

    let output_image =
        OutputImageWriter::new(output_dir.to_path_buf(), temp_dir.clone()).context("Construct OutputImageWriter")?;

    rayon::ThreadPoolBuilder::new()
        .thread_name(|i| format!("thread-{}", i))
        .num_threads(args.concurrency.unwrap_or_default())
        .build_global()?;
    info!("Using {} threads", rayon::current_num_threads());
    let platform_matcher = PlatformMatcher::from_glob(args.platform)?;

    let results = match args.source {
        Location::Oci(path) => {
            info!("Reading images from OCI directory: {}", path.display());
            let images = LocalOciImage::from_oci_directory(path, &platform_matcher)?;
            handle_input_images(images, &temp_dir, &output_image, target_size, args.compression_level)?
        }
        Location::Docker(reference) => {
            info!("Reading images registry: {}", reference);
            let runtime = tokio::runtime::Runtime::new()?;
            let images = RemoteImage::create_remote_images(runtime.handle(), reference, &platform_matcher)?;
            handle_input_images(images, &temp_dir, &output_image, target_size, args.compression_level)?
        }
    };

    if !args.keep_temp_files {
        std::fs::remove_dir_all(&temp_dir)?;
    }

    info!("Wrote {} images to {}:", results.len(), output_dir.display());
    for (_, _, written_image) in &results {
        let total_size = written_image.layers.iter().map(|l| l.compressed_file_size).sum::<u64>();
        info!(
            "Written image {} - {:#.1}:",
            written_image.platform,
            display_bytes(total_size)
        );
        for layer in &written_image.layers {
            info!(" - {}", layer);
        }
    }

    let manifests = results
        .into_iter()
        .map(|(size, hash, stats)| (size, hash.clone(), stats))
        .sorted_by_key(|(_, _, stats)| stats.platform.to_string())
        .collect::<Vec<_>>();

    output_image.write_image_index(&manifests)?;
    info!("Completed");
    Ok(())
}

fn handle_input_images<T: InputImage>(
    images: Vec<T>,
    temp_dir: &Path,
    output_image: &OutputImageWriter,
    target_size: Byte,
    compression_level: i32,
) -> anyhow::Result<Vec<(u64, Sha256Digest, WrittenImageStats)>> {
    info!("Found {} images", images.len());
    for image in &images {
        info!(" - {} - digest: {}", image.platform(), image.image_digest());
    }

    let images = progress_parallel_collect::<Vec<_>, _>(
        "Loading and merging",
        images.into_par_iter().map(|input_image| {
            let image_digest = input_image.image_digest();
            let platform_key = input_image.platform().file_key()?;
            let combined_path = temp_dir.join(format!("combined-{platform_key}-{image_digest}.tar"));
            let image_items = load_and_merge_image(&input_image, &combined_path)?;
            Ok((input_image, image_items))
        }),
    )?;
    info!(
        "Loaded and merged {} images - {} items in total",
        images.len(),
        images.iter().map(|(_, v)| v.total_items).sum::<usize>()
    );
    let images_with_content = images
        .iter()
        .map(|(input_image, image_items)| {
            let image_content = image_items.get_image_content()?;
            Ok((input_image, image_content))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let all_image_items = images_with_content
        .into_iter()
        .flat_map(|(input_image, items)| items.into_iter().map(move |v| (input_image, v)))
        .collect::<Vec<_>>();

    info!(
        "Read {} files from images, hashing and compressing files",
        all_image_items.len()
    );

    let hashed_items = progress_parallel_collect::<Vec<_>, _>(
        "Hashing and compressing",
        all_image_items.into_par_iter().map_init(
            || ImageItem::create_compressor(compression_level).unwrap(),
            |compressor, (input_image, (path, header, content))| {
                let item =
                    ImageItem::from_path_and_header(path, header, content, compressor).map(|v| (v.path.clone(), v))?;
                Ok((input_image, item))
            },
        ),
    )?;
    let file_count = hashed_items.iter().filter(|(_, (_, item))| item.raw_size > 0).count();
    let unique_file_count = hashed_items
        .iter()
        .filter_map(|(_, (_, item))| if item.raw_size > 0 { Some(item.hash) } else { None })
        .unique()
        .count();
    info!(
        "Hashed {} items from images - {} non-empty files, {} unique files, {} duplicate files",
        hashed_items.len(),
        file_count,
        unique_file_count,
        file_count - unique_file_count
    );

    let all_image_items: Vec<(_, HashMap<_, _>)> = hashed_items
        .into_iter()
        .into_group_map()
        .into_iter()
        .map(|(input_image, items)| {
            let items = items.into_iter().collect();
            (input_image, items)
        })
        .collect();
    let total_item_count: usize = all_image_items.iter().map(|(_, map)| map.len()).sum();
    info!("Packing {} files into layers", total_item_count);
    let output_layers = all_image_items
        .iter()
        .map(|(input_image, items)| {
            let output_layer = OutputLayers::pack_items(items, 4096, target_size.as_u64())
                .with_context(|| format!("Packing layers for {}", input_image))?;
            Ok((input_image, output_layer))
        })
        .collect::<anyhow::Result<Vec<_>>>()?;

    let mut flattened_layers = output_layers
        .iter()
        .flat_map(|(image, layers)| layers.all_layers().iter().map(move |layer| (image, layer)))
        .collect::<Vec<_>>();

    // Shuffle the layers to avoid any bias in the order of the layers
    // We re-sort the layers in write_oci_image
    let mut small_rng = SmallRng::from_entropy();
    flattened_layers.shuffle(&mut small_rng);

    info!("Produced {} total layers", flattened_layers.len());

    let written_layers = progress_parallel_collect::<Vec<_>, _>(
        "Writing Layers",
        flattened_layers.into_par_iter().map(|(image, layer)| {
            let raw_size = display_bytes(layer.raw_size());
            let span = info_span!(
                "write_layer",
                items = layer.len(),
                raw_size = format_args!("{:#.1}", raw_size)
            );
            let result = span.in_scope(|| {
                output_image
                    .write_layer(layer, compression_level, image.image_digest())
                    .with_context(|| format!("Write layer {layer}"))
            })?;
            Ok((image, result))
        }),
    )?;
    info!(
        "Wrote {} layers, writing config and finalizing image:",
        written_layers.len()
    );
    let written_layers_map = written_layers.into_iter().into_group_map();
    written_layers_map
        .into_iter()
        .map(|(image, layers)| {
            output_image
                .write_oci_image(image.config().clone(), layers, image.platform())
                .context("Write Image")
        })
        .collect::<anyhow::Result<Vec<_>>>()
}

#[instrument(skip_all, fields(image = %input_image))]
fn load_and_merge_image(input_image: &impl InputImage, combined_path: &Path) -> anyhow::Result<ImageItems<Mmap>> {
    let combined_output_file = File::options()
        .create(true)
        .truncate(true)
        .write(true)
        .open(combined_path)
        .with_context(|| format!("Opening file {combined_path:?}"))?;
    let mut combiner = LayerCombiner::new(combined_output_file);
    let layer_iterator = input_image.layers_from_manifest()?;
    for input_layer in progress::progress_iter("Merging Layers", layer_iterator) {
        let mut input_layer = input_layer?;
        let entries = progress::spinner_iter("Merging Entries", input_layer.entries()?);
        combiner.merge_entries(entries)?;
    }

    let total_items = combiner.finish()?;
    ImageItems::from_file(combined_path, total_items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer_combiner::LayerCombiner;

    use crate::test_utils::build_layer;

    #[test]
    fn test_multiple_layers() {
        let layer_1 = build_layer()
            .with_files(&[
                ("one.txt", b"content1"),
                ("two.txt", b"content2"),
                ("three.txt", b"content3"),
                ("four.txt", b"content4"),
            ])
            .build();

        let layer_2 = build_layer()
            .with_files(&[
                ("five.txt", b"content5"),
                ("six.txt", b"content6"),
                ("seven.txt", b"content7"),
                ("eight.txt", b"content8"),
            ])
            .build();

        let layer_3 = build_layer()
            .with_files(&[
                ("one.txt", b"new content 1"),
                ("five.txt", b"new content 2"),
                ("nine.txt", b"new content 3"),
            ])
            .build();

        let mut data = vec![];
        let mut combiner = LayerCombiner::new(&mut data);
        combiner.merge_layer(layer_3).unwrap();
        combiner.merge_layer(layer_2).unwrap();
        combiner.merge_layer(layer_1).unwrap();
        let total_items = combiner.finish().unwrap();
        assert_eq!(total_items, 9);

        let items = ImageItems::from_data(data, 9);
        let content = items.get_image_content().unwrap();
        let image_items = ImageItem::items_from_data(content, 1).unwrap();
        assert_eq!(image_items.len(), 9);
        let layers = OutputLayers::pack_items(&image_items, 4096, 1024 * 1024 * 250).unwrap();
        assert_eq!(layers.len(), 1);
    }
}
