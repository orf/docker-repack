use std::ops::Range;

pub fn byte_range_chunks(size: u64, chunk_size: u64) -> impl Iterator<Item = Range<u64>> {
    (0..size)
        .step_by(chunk_size as usize)
        .map(move |byte_start| {
            let byte_end = (byte_start + chunk_size).min(size);
            byte_start..byte_end
        })
}
