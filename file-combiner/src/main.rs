use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    index_path: PathBuf,
}

fn main() {
    println!("Hello, world!");
}
