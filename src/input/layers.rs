use std::fmt::{Debug, Display, Formatter};
use std::io::Read;
use tar::{Archive, Entry};

pub struct InputLayer<T: Read> {
    pub name: String,
    archive: Archive<T>,
}

impl<T: Read> InputLayer<T> {
    pub fn new(name: String, reader: T) -> anyhow::Result<InputLayer<T>> {
        let archive = Archive::new(reader);
        Ok(Self { name, archive })
    }

    pub fn entries(&mut self) -> anyhow::Result<impl Iterator<Item = std::io::Result<Entry<T>>>> {
        Ok(self.archive.entries()?)
    }
}

impl<T: Read> Display for InputLayer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

impl<T: Read> Debug for InputLayer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::Compression;
    use crate::test_utils::{add_dir, add_file, setup_tar};

    #[test]
    fn input_layer_entries() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/file.txt", b"hello world");
        let vec = tar_1.into_inner().unwrap();

        let compressed_reader = Compression::Raw.new_reader(vec.as_slice()).unwrap();
        let mut input_layer = InputLayer::new("test".to_string(), compressed_reader).unwrap();

        assert_eq!(input_layer.to_string(), "test");
        assert_eq!(input_layer.entries().unwrap().count(), 2);
    }
}
