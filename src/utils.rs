use byte_unit::{AdjustedByte, Byte, UnitType};
use indicatif::{MultiProgress, ProgressBar, ProgressFinish, ProgressStyle};
use std::borrow::Cow;

#[cfg(feature = "split_files")]
use std::ops::Range;
use std::time::Duration;

#[cfg(feature = "split_files")]
pub fn byte_range_chunks(size: u64, chunk_size: u64) -> impl Iterator<Item = Range<u64>> {
    (0..size).step_by(chunk_size as usize).map(move |byte_start| {
        let byte_end = (byte_start + chunk_size).min(size);
        byte_start..byte_end
    })
}

pub fn display_bytes(size: u64) -> AdjustedByte {
    Byte::from(size).get_appropriate_unit(UnitType::Binary)
}

pub fn create_pbar(
    progress: &MultiProgress,
    size: u64,
    message: impl Into<Cow<'static, str>>,
    bytes: bool,
) -> ProgressBar {
    let template = if !bytes {
        ProgressStyle::with_template("{msg:>10} {percent}% {wide_bar} {per_sec} [{human_pos}/{human_len}]").unwrap()
    } else {
        ProgressStyle::with_template(
            "{msg:>10} {percent}% {wide_bar} {decimal_bytes_per_sec} [{decimal_bytes}/{decimal_total_bytes}]",
        )
        .unwrap()
    };
    let pbar = progress.add(
        ProgressBar::new(size)
            .with_style(template)
            .with_finish(ProgressFinish::AndClear)
            .with_message(message),
    );
    pbar.enable_steady_tick(Duration::from_millis(100));
    pbar
}
