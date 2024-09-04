use memmap2::Mmap;
use sha2::Digest;
use std::fs::File;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use tar::{Archive, Header};
use zstd::bulk::Compressor;
use zstd::zstd_safe;

#[cfg(test)]
use std::collections::HashMap;

const EMPTY_SHA: [u8; 32] = [
    227, 176, 196, 66, 152, 252, 28, 20, 154, 251, 244, 200, 153, 111, 185, 36, 39, 174, 65, 228, 100, 155, 147, 76,
    164, 149, 153, 27, 120, 82, 184, 85,
];

pub struct ImageItems<T: AsRef<[u8]>> {
    data: T,
    pub total_items: usize,
}

impl ImageItems<Mmap> {
    pub fn from_file(path: impl AsRef<Path>, total_items: usize) -> anyhow::Result<ImageItems<Mmap>> {
        let combined_input_file = File::options().read(true).open(path)?;
        let data = unsafe { memmap2::MmapOptions::new().map(&combined_input_file) }?;
        assert_ne!(data.len(), 0);

        Ok(ImageItems { total_items, data })
    }
}

impl<'a, T: AsRef<[u8]> + 'a> ImageItems<T> {
    #[cfg(test)]
    pub fn from_data(data: T, total_items: usize) -> ImageItems<T> {
        assert_ne!(data.as_ref().len(), 0);
        ImageItems { total_items, data }
    }
    pub fn get_image_content(&self) -> anyhow::Result<Vec<(PathBuf, Header, &[u8])>> {
        let data = self.data.as_ref();
        let seek = Cursor::new(data);
        let mut archive = Archive::new(seek);

        let mut items = Vec::with_capacity(self.total_items);

        for entry in archive.entries_with_seek()? {
            let entry = entry?;
            let start = entry.raw_file_position() as usize;
            let end = start + entry.size() as usize;
            let content = &data[start..end];
            debug_assert_eq!(content.len(), entry.size() as usize);
            let path = entry.path()?.to_path_buf();
            let header = entry.header().clone();
            items.push((path, header, content));
        }

        debug_assert_eq!(items.len(), self.total_items);
        Ok(items)
    }
}

#[derive(Debug)]
pub struct ImageItem<'a> {
    pub path: PathBuf,
    pub header: Header,
    pub content: &'a [u8],
    pub hash: [u8; 32],
    pub compressed_size: u64,
    pub raw_size: u64,
}

impl<'a> ImageItem<'a> {
    pub fn create_compressor(compression_level: i32) -> anyhow::Result<Compressor<'a>> {
        let mut compressor = Compressor::new(compression_level)?;
        compressor.set_parameter(zstd_safe::CParameter::ChecksumFlag(false))?;
        compressor.set_parameter(zstd_safe::CParameter::ContentSizeFlag(false))?;
        compressor.set_parameter(zstd_safe::CParameter::Format(zstd_safe::FrameFormat::Magicless))?;
        Ok(compressor)
    }

    pub fn from_path_and_header(
        path: PathBuf,
        header: Header,
        content: &'a [u8],
        compressor: &mut Compressor,
    ) -> anyhow::Result<Self> {
        let raw_size = content.len() as u64;
        let (compressed_size, hash) = if content.is_empty() {
            (0, EMPTY_SHA)
        } else {
            let compressed = compressor.compress(content)?;
            let header_size =
                unsafe { zstd_safe::zstd_sys::ZSTD_frameHeaderSize(compressed.as_ptr() as *const _, compressed.len()) };
            let compressed_size = (compressed.len() - header_size) as u64;
            let hash = sha2::Sha256::digest(content).into();
            (compressed_size, hash)
        };

        Ok(Self {
            path,
            header,
            content,
            hash,
            compressed_size,
            raw_size,
        })
    }

    #[cfg(test)]
    pub fn items_from_data(
        items: Vec<(PathBuf, Header, &[u8])>,
        compression_level: i32,
    ) -> anyhow::Result<HashMap<PathBuf, ImageItem>> {
        let mut compressor = ImageItem::create_compressor(compression_level)?;
        let mut image_items = Vec::with_capacity(items.len());
        for (path, header, content) in items {
            let item = ImageItem::from_path_and_header(path, header, content, &mut compressor)?;
            image_items.push((item.path.clone(), item));
        }
        Ok(image_items.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{add_dir, add_file, setup_tar};
    use std::path::Path;

    #[test]
    fn test_from_vec() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/foo.txt", b"hello world");
        add_file(&mut tar_1, "test/foo2.txt", b"hello world 2");
        let data = tar_1.into_inner().unwrap();

        let items = ImageItems::from_data(data, 3);
        let content = items.get_image_content().unwrap();
        let items = ImageItem::items_from_data(content, 1).unwrap();
        assert_eq!(items.len(), 3);

        assert_eq!(
            items[Path::new("test/foo.txt")].hash.to_vec(),
            const_hex::const_decode_to_array::<32>(b"b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9")
                .unwrap()
        );
        assert_eq!(
            items[Path::new("test/foo2.txt")].hash.to_vec(),
            const_hex::const_decode_to_array::<32>(b"ed12932f3ef94c0792fbc55263968006e867e522cf9faa88274340a2671d4441")
                .unwrap()
        )
    }

    #[test]
    fn test_compressed_size() {
        let mut tar_1 = setup_tar();
        add_file(&mut tar_1, "foo.txt", b"hihi");
        let data = tar_1.into_inner().unwrap();
        let items = ImageItems::from_data(data, 1);
        let content = items.get_image_content().unwrap();
        let items = ImageItem::items_from_data(content, 1).unwrap();
        let item = &items[&PathBuf::from("foo.txt")];
        assert_eq!(item.compressed_size, 3);
    }
}
