#[cfg(test)]
use crate::input::layers::InputLayer;
use memchr::memmem;
use std::collections::HashSet;
use std::io::{Read, Write};
use tar::{Builder, Entry};
use zstd::zstd_safe::WriteBuf;

const WHITEOUT_OPAQUE: &[u8] = b".wh..wh..opq";
const WHITEOUT_PREFIX: &[u8] = b".wh.";

pub struct LayerCombiner<T: Write> {
    archive: Builder<T>,
    items: HashSet<Vec<u8>>,
    whiteout_directories: HashSet<Vec<u8>>,
    whiteout_files: HashSet<Vec<u8>>,
}

impl<T: Write> LayerCombiner<T> {
    pub fn new(output: T) -> Self {
        let archive = Builder::new(output);
        Self {
            archive,
            items: HashSet::new(),
            whiteout_directories: HashSet::new(),
            whiteout_files: HashSet::new(),
        }
    }

    fn add_entry(&mut self, entry: Entry<impl Read>) -> anyhow::Result<()> {
        let entry_path = entry.path_bytes().to_vec();
        if entry_path.ends_with(WHITEOUT_OPAQUE) {
            let directory = &entry_path[..entry_path.len() - WHITEOUT_OPAQUE.len()];
            self.whiteout_directories.insert(directory.to_vec());
        } else if let Some(whiteout) = memmem::rfind(&entry_path, WHITEOUT_PREFIX) {
            let whiteout_file_name = &entry_path[whiteout + WHITEOUT_PREFIX.len()..];
            let whiteout_directory = &entry_path[..whiteout];
            let whiteout_path = [whiteout_directory, whiteout_file_name].concat();
            self.whiteout_files.insert(whiteout_path);
        } else {
            self.archive.append(&entry.header().clone(), entry)?;
            self.items.insert(entry_path);
        }
        Ok(())
    }

    fn should_add_path(&mut self, path: &[u8]) -> bool {
        let in_whiteout_files = self.whiteout_files.contains(path);
        let in_items = self.items.contains(path);
        let in_whiteout_directories = self.whiteout_directories.iter().any(|dir| path.starts_with(dir));

        !in_whiteout_files && !in_items && !in_whiteout_directories
    }

    #[cfg(test)]
    pub fn merge_layer(&mut self, mut layer: InputLayer<impl Read>) -> anyhow::Result<()> {
        self.merge_entries(layer.entries()?)
    }

    pub fn merge_entries<'a>(
        &mut self,
        entries: impl Iterator<Item = std::io::Result<Entry<'a, impl Read + 'a>>>,
    ) -> anyhow::Result<()> {
        for entry in entries {
            let entry = entry?;
            let entry_path = entry.path_bytes();
            let path = entry_path.as_slice();

            if self.should_add_path(path) {
                self.add_entry(entry)?
            }
        }
        Ok(())
    }

    pub fn finish(mut self) -> anyhow::Result<usize> {
        self.archive.finish()?;
        Ok(self.items.len())
    }

    #[cfg(test)]
    fn into_inner(self) -> anyhow::Result<(T, usize)> {
        Ok((self.archive.into_inner()?, self.items.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::Compression;
    use crate::test_utils::{add_dir, add_file, build_layer, read_tar_entries_content, setup_tar};
    use std::path::Path;

    fn make_input_layer(builder: Builder<Vec<u8>>) -> InputLayer<impl Read> {
        let finished = builder.into_inner().unwrap();
        assert_ne!(finished.len(), 0);
        let reader = std::io::Cursor::new(finished);
        let compressed_reader = Compression::Raw.new_reader(reader).unwrap();
        InputLayer::new("test".to_string(), compressed_reader).unwrap()
    }

    #[test]
    fn test_file_whiteout() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/.wh.foo.txt", b"");
        let input_layer_1 = make_input_layer(tar_1);

        let mut tar_2 = setup_tar();
        add_dir(&mut tar_2, "test/");
        add_file(&mut tar_2, "test/foo.txt", b"hello world");
        let input_layer_2 = make_input_layer(tar_2);

        let mut combiner = LayerCombiner::new(vec![]);
        combiner.merge_layer(input_layer_1).unwrap();
        combiner.merge_layer(input_layer_2).unwrap();

        assert_eq!(combiner.whiteout_files, HashSet::from([b"test/foo.txt".to_vec()]));
        assert_eq!(combiner.items.len(), 1);
    }

    #[test]
    fn test_opaque_whiteout() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/.wh..wh..opq", b"");
        let input_layer_1 = make_input_layer(tar_1);

        let mut tar_2 = setup_tar();
        add_dir(&mut tar_2, "test/");
        add_file(&mut tar_2, "test/new-file.txt", b"hello world");
        add_file(&mut tar_2, "test/foo.txt", b"hello world");
        let input_layer_2 = make_input_layer(tar_2);

        let mut combiner = LayerCombiner::new(vec![]);
        combiner.merge_layer(input_layer_1).unwrap();
        combiner.merge_layer(input_layer_2).unwrap();

        assert_eq!(combiner.whiteout_directories, HashSet::from([b"test/".to_vec()]));
        assert_eq!(combiner.items.len(), 1);
    }

    #[test]
    fn test_multiple_layers() {
        let layer_1 = build_layer()
            .with_files(&[
                ("one.txt", b"content1"),
                ("two.txt", b"content2"),
                ("three.txt", b"content3"),
                ("four.txt", b"content4"),
            ])
            .build();

        let layer_2 = build_layer()
            .with_files(&[
                ("five.txt", b"content5"),
                ("six.txt", b"content6"),
                ("seven.txt", b"content7"),
                ("eight.txt", b"content8"),
            ])
            .build();

        let layer_3 = build_layer()
            .with_files(&[
                ("one.txt", b"new content 1"),
                ("five.txt", b"new content 2"),
                ("nine.txt", b"new content 3"),
            ])
            .build();

        let mut output = vec![];
        let mut combiner = LayerCombiner::new(&mut output);
        combiner.merge_layer(layer_3).unwrap();
        combiner.merge_layer(layer_2).unwrap();
        combiner.merge_layer(layer_1).unwrap();
        let (data, total) = combiner.into_inner().unwrap();
        assert_eq!(total, 9);
        let entries = read_tar_entries_content(data);
        assert_eq!(entries.len(), 9);
        assert_eq!(entries[Path::new("one.txt")], b"new content 1");
        assert_eq!(entries[Path::new("five.txt")], b"new content 2");
    }
}
