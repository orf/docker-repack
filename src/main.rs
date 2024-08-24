use crate::io::image::reader::ImageReader;
use anyhow::Context;
use byte_unit::Byte;
use clap::Parser;
use clap_num::number_range;
use cmd::repack;
use globset::{Glob, GlobSet};
use indicatif::MultiProgress;
use std::path::PathBuf;
use tracing::{info, warn};
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
    image_dir: PathBuf,
    output_dir: PathBuf,

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

    #[arg(long)]
    keep_temp_files: bool
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
    let image_dir = args.image_dir;
    info!("Reading image from {:?}", image_dir);

    let image = ImageReader::from_dir(&image_dir)
        .with_context(|| format!("Error opening image {image_dir:?}"))?;
    std::fs::create_dir_all(&args.output_dir)?;

    let progress = MultiProgress::new();

    let exclude = create_glob_set(args.exclude)?;
    repack::repack(
        progress,
        image,
        args.output_dir,
        args.target_size,
        args.split_files,
        exclude,
        args.compression_level,
        args.skip_compression,
        args.keep_temp_files
    )
}

fn create_glob_set(exclude: Option<Vec<globset::Glob>>) -> anyhow::Result<Option<GlobSet>> {
    let glob_set = match exclude {
        None => None,
        Some(globs) => {
            let mut builder = globset::GlobSetBuilder::new();
            for glob in globs {
                let mut glob = glob;
                if glob.glob().starts_with('/') {
                    warn!("Stripping / prefix from glob {} - globs should be relative to /", glob);
                    glob = Glob::new(&glob.glob()[1..])?;
                }
                builder.add(glob);
            }
            Some(builder.build()?)
        }
    };

    Ok(glob_set)
}
