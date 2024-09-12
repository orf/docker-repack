use oci_spec::image::Digest;
use std::fmt::{Debug, Display, Formatter};
use std::io::Read;
use tar::{Archive, Entry};

pub struct InputLayer<T: Read> {
    pub name: Digest,
    archive: Archive<T>,
}

impl<T: Read> InputLayer<T> {
    pub fn new(name: Digest, reader: T) -> anyhow::Result<InputLayer<T>> {
        let archive = Archive::new(reader);
        Ok(Self { name, archive })
    }

    pub fn entries(&mut self) -> anyhow::Result<impl Iterator<Item = std::io::Result<Entry<T>>>> {
        Ok(self.archive.entries()?)
    }
}

impl<T: Read> Display for InputLayer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.name))
    }
}

impl<T: Read> Debug for InputLayer<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!("{}", self.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compression::Compression;
    use crate::test_utils::{add_dir, add_file, setup_tar};
    use std::str::FromStr;

    #[test]
    fn input_layer_entries() {
        let mut tar_1 = setup_tar();
        add_dir(&mut tar_1, "test/");
        add_file(&mut tar_1, "test/file.txt", b"hello world");
        let vec = tar_1.into_inner().unwrap();

        let compressed_reader = Compression::Raw.new_reader(vec.as_slice()).unwrap();
        let mut input_layer = InputLayer::new(
            Digest::from_str("sha256:0d90d93a5cab3fd2879040420c7b7e4958aee8997fef78e9a5dd80cb01f3bd9c").unwrap(),
            compressed_reader,
        )
        .unwrap();

        assert_eq!(
            input_layer.to_string(),
            "sha256:0d90d93a5cab3fd2879040420c7b7e4958aee8997fef78e9a5dd80cb01f3bd9c"
        );
        assert_eq!(input_layer.entries().unwrap().count(), 2);
    }
}
