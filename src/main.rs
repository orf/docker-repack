use crate::io::image::reader::ImageReader;
use anyhow::Context;
use byte_unit::Byte;
use clap::{Parser, Subcommand};
use clap_num::number_range;
use cmd::repack;
use globset::GlobSet;
use indicatif::MultiProgress;
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, EnvFilter};
use zstd::zstd_safe::CompressionLevel;

mod content;
mod io;
mod utils;
//mod size_breakdown;
mod cmd;
mod packing;
mod tar_item;

fn parse_compression_level(s: &str) -> Result<CompressionLevel, String> {
    let range = zstd::compression_level_range();
    number_range(s, *range.start(), *range.end())
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    root_dir: PathBuf,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    Repack {
        #[arg(short, long)]
        target_size: Byte,

        #[arg(short, long, value_parser = parse_compression_level, default_value = "7")]
        compression_level: CompressionLevel,

        #[arg(short, long)]
        skip_compression: bool,

        #[arg(short, long)]
        exclude: Option<Vec<globset::Glob>>,

        #[arg(short, long)]
        split_files: Option<Byte>,
    },
}

fn main() -> anyhow::Result<()> {
    let filter = EnvFilter::builder()
        .with_default_directive("docker_repack=info".parse().unwrap())
        .from_env_lossy();
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(filter)
        .init();

    let args = Args::parse();
    let image_dir = args.root_dir.join("image");
    info!("Reading image from {:?}", image_dir);
    let image = ImageReader::from_dir(image_dir).context("Error opening image")?;
    let new_image_dir = args.root_dir.join("image_new");
    std::fs::create_dir_all(&new_image_dir)?;

    let progress = MultiProgress::new();
    match args.command {
        Command::Repack {
            target_size,
            split_files,
            compression_level,
            exclude,
            skip_compression,
        } => {
            let exclude = create_glob_set(exclude)?;
            repack::repack(
                progress,
                image,
                new_image_dir,
                target_size,
                split_files,
                exclude,
                compression_level,
                skip_compression,
            )
        } // Command::LargestFiles { limit } => inspect::largest_files(progress, image, exclude, limit),
    }
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
