use crate::image_parser::{TarItem, TarItemChunk};
use std::fmt::Write;

const SCRIPT: &'static str = include_str!("./combine_files.sh");

pub fn generate_combining_script(
    chunked_files: &Vec<(&TarItem, Vec<TarItemChunk>)>,
) -> anyhow::Result<String> {
    let mut content = SCRIPT.to_string();
    for (item, chunks) in chunked_files {
        writeln!(content, "# {}", item.content_hash_hex().unwrap().as_str())?;
        write!(content, "combine \"{}\" ", item.path.display())?;
        for chunk in chunks {
            write!(content, "\"{}\" ", chunk.dest_path().display())?;
        }
        writeln!(content)?;
    }
    Ok(content)
}

pub fn generate_combining_index(
    chunked_files: &Vec<(&TarItem, Vec<TarItemChunk>)>,
) -> anyhow::Result<String> {
    let mut content = "".to_string();
    for (item, chunks) in chunked_files {
        write!(content, "{}\t", item.content_hash_hex().unwrap().as_str())?;
        write!(content, "{}\t", item.path.display())?;
        for chunk in chunks {
            write!(content, "\t{}", chunk.dest_path().display())?;
        }
        writeln!(content)?;
    }
    Ok(content)
}
