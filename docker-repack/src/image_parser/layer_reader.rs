use crate::image_parser::image_reader::SourceLayerID;
use flate2::read::GzDecoder;
use indicatif::{MultiProgress, ProgressBar, ProgressBarIter, ProgressFinish, ProgressStyle};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use tar::Archive;

pub struct Layer {
    pub id: SourceLayerID,
    pub path: PathBuf,
    pub size: u64,
}

pub fn progress_reader(
    progress: &MultiProgress,
    size: u64,
    file: BufReader<File>,
    message: String,
) -> ProgressBarIter<BufReader<File>> {
    progress
        .add(
            ProgressBar::new(size)
                .with_style(
                    ProgressStyle::with_template(
                        "{msg:>10} {wide_bar} {binary_bytes}/{binary_total_bytes}",
                    )
                    .unwrap(),
                )
                .with_finish(ProgressFinish::AndClear)
                .with_message(message),
        )
        .wrap_read(file)
}

impl Layer {
    pub fn get_progress_reader(
        &self,
        progress: Option<&MultiProgress>,
    ) -> anyhow::Result<Archive<impl Read>> {
        let file = File::open(&self.path)?;
        let file = BufReader::new(file);
        let writer = match progress {
            None => ProgressBar::hidden().wrap_read(file),
            Some(multi_progress) => progress_reader(
                multi_progress,
                self.size,
                file,
                format!("Reading {}", self.id),
            ),
        };

        let decoder = GzDecoder::new(writer);
        Ok(Archive::new(decoder))
    }
}
