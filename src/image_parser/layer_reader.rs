use crate::image_parser::image::LayerID;
use flate2::read::GzDecoder;
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use tar::Archive;

pub struct Layer {
    pub id: LayerID,
    pub path: PathBuf,
    pub size: u64,
    pub digest: String,
    pub regular_file_count: usize,
}

impl Layer {
    pub fn get_progress_reader(
        &self,
        progress: Option<&MultiProgress>,
    ) -> anyhow::Result<Archive<impl Read>> {
        let bar = match progress {
            None => ProgressBar::hidden(),
            Some(multi_progress) => multi_progress.add(
                ProgressBar::new(self.size)
                    .with_style(
                        ProgressStyle::with_template(
                            "{wide_bar} {binary_bytes}/{binary_total_bytes}",
                        )
                        .unwrap(),
                    )
                    .with_finish(ProgressFinish::AndClear),
            ),
        };
        let file = File::open(&self.path)?;
        let file = bar.wrap_read(file);
        let file = BufReader::new(file);
        let decoder = GzDecoder::new(file);
        Ok(Archive::new(decoder))
    }
}
