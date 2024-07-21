// use crate::image_parser::TarItem;
// use anyhow::Context;
// use byte_unit::{Byte, UnitType};
// use pack_it_up::Pack;
// use std::fmt::{Display, Formatter};
// use std::io::Read;
// use tar::Entry;
//
//
// pub struct ZstdCompressor<'a> {
//     compressor: Compressor<'a>,
//     compression_buffer: Vec<u8>,
//     data_buffer: Vec<u8>,
// }
//
// impl ZstdCompressor<'_> {
//     pub fn new() -> Self {
//         let compressor = Compressor::new(ZSTD_TEST_COMPRESSION_LEVEL).unwrap();
//         Self {
//             compressor,
//             compression_buffer: Vec::new(),
//             data_buffer: Vec::new(),
//         }
//     }
//
//     pub fn compress(
//         &mut self,
//         tar_item: TarItem,
//         entry: &mut Entry<impl Read>,
//     ) -> anyhow::Result<CompressionResult> {
//         self.data_buffer.clear();
//         self.data_buffer.reserve(entry.size() as usize);
//
//         entry
//             .read_to_end(&mut self.data_buffer)
//             .context("Error reading entry")?;
//         let buffer_len = zstd_safe::compress_bound(self.data_buffer.len());
//         self.compression_buffer.clear();
//         self.compression_buffer.reserve(buffer_len);
//
//         let compressed_size = self
//             .compressor
//             .compress_to_buffer(&self.data_buffer, &mut self.compression_buffer)
//             .context("Error compressing zstd buffer")? as u64;
//
//         let compression_ratio =
//             ((tar_item.raw_size as f64 / compressed_size as f64) * 100f64) as u64;
//         Ok(CompressionResult {
//             tar_item,
//             compressed_size,
//             compression_ratio,
//         })
//     }
// }
//
// #[derive(Debug, Clone)]
// pub struct CompressionResult {
//     pub tar_item: TarItem,
//     pub compressed_size: u64,
//     pub compression_ratio: u64,
// }
//
// impl Display for CompressionResult {
//     fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
//         let compressed_size =
//             Byte::from(self.compressed_size).get_appropriate_unit(UnitType::Decimal);
//         let raw_size = Byte::from(self.tar_item.raw_size).get_appropriate_unit(UnitType::Decimal);
//         write!(f, "CompressedTarItem {{ path: {:?}, raw_size: {:#.1}, compressed_size: {:#.1}, ratio: {:#.1}% }}", self.tar_item.path, raw_size, compressed_size, self.compression_ratio)
//     }
// }
//
// impl Pack for &CompressionResult {
//     fn size(&self) -> usize {
//         self.compressed_size as usize
//     }
// }
