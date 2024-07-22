use std::path::PathBuf;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    index_path: PathBuf,
}

fn main() {
    println!("Hello, world!");
}
