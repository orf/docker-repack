use indicatif::{MultiProgress, ProgressBarIter};
use std::io::Read;

pub fn progress_reader<T: Read>(
    progress: &MultiProgress,
    size: u64,
    file: T,
    message: String,
) -> ProgressBarIter<T> {
    crate::utils::create_pbar(progress, size, message, true).wrap_read(file)
}
