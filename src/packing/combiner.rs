use crate::io::layer::writer::LayerWriter;
use crate::tar_item::{TarItem, TarItemChunk};
use itertools::Itertools;
use std::fmt::Write;
use std::ops::Range;
use std::path::{Path, PathBuf};

const SCRIPT: &str = include_str!("./combine_files.sh");

#[derive(Debug)]
pub struct FileCombiner<'a> {
    chunked_files: Vec<(&'a TarItem, Vec<TarItemChunk<'a>>)>,
}

impl<'a> FileCombiner<'a> {
    pub fn new() -> Self {
        Self { chunked_files: vec![] }
    }

    pub fn is_empty(&self) -> bool {
        self.chunked_files.is_empty()
    }

    pub fn len(&self) -> usize {
        self.chunked_files.len()
    }

    #[cfg(feature = "split_files")]
    pub fn add_chunked_file(&mut self, item: &'a TarItem, chunks: Vec<TarItemChunk<'a>>) {
        self.chunked_files.push((item, chunks))
    }

    pub fn write_to_image(self, directory: &Path, layer_writer: &mut LayerWriter) -> anyhow::Result<Vec<String>> {
        let script_path = directory.join("combine.sh");
        layer_writer.new_directory(directory, 0x755)?;
        layer_writer.new_item(
            &directory.join("index.txt"),
            0x755,
            self.generate_combining_text_index()?.as_bytes(),
        )?;
        layer_writer.new_item(
            &directory.join("index.json"),
            0x755,
            self.generate_combining_json_index()?.as_bytes(),
        )?;
        layer_writer.new_item(&script_path, 0x4777, self.generate_combining_script()?.as_bytes())?;
        Ok(vec![format!("/{}", script_path.display())])
    }

    fn generate_combining_script(&self) -> anyhow::Result<String> {
        let mut content = SCRIPT.to_string();
        for (item, chunks) in &self.chunked_files {
            writeln!(content, "# {}", item.content_hash_hex().unwrap().as_str())?;
            write!(content, "combine \"{}\" ", item.path.display())?;
            for chunk in chunks {
                write!(content, "\"{}\" ", chunk.dest_path().display())?;
            }
            writeln!(content)?;
        }
        writeln!(content)?;
        writeln!(content, "completed")?;
        Ok(content)
    }

    fn generate_combining_text_index(&self) -> anyhow::Result<String> {
        let mut content = "".to_string();
        for (item, chunks) in &self.chunked_files {
            write!(content, "{}\t", item.content_hash_hex().unwrap().as_str())?;
            write!(content, "{}\t", item.path.display())?;
            for chunk in chunks {
                write!(content, "\t{}", chunk.dest_path().display())?;
            }
            writeln!(content)?;
        }
        Ok(content)
    }

    fn generate_combining_json_index(&self) -> anyhow::Result<String> {
        let index = JsonIndex {
            files: self
                .chunked_files
                .iter()
                .map(|(item, chunks)| JsonIndexFile {
                    path: &item.path,
                    hash: item.content_hash_hex().unwrap().to_string(),
                    size: item.size,
                    chunks: chunks
                        .iter()
                        .map(|chunk| JsonIndexFileChunk {
                            path: chunk.dest_path(),
                            byte_range: &chunk.byte_range,
                        })
                        .collect_vec(),
                })
                .collect_vec(),
        };
        Ok(serde_json::to_string_pretty(&index)?)
    }
}

#[derive(serde::Serialize)]
struct JsonIndex<'a> {
    files: Vec<JsonIndexFile<'a>>,
}

#[derive(serde::Serialize)]
struct JsonIndexFile<'a> {
    path: &'a PathBuf,
    hash: String,
    size: u64,
    chunks: Vec<JsonIndexFileChunk<'a>>,
}

#[derive(serde::Serialize)]
struct JsonIndexFileChunk<'a> {
    path: PathBuf,
    byte_range: &'a Range<u64>,
}
