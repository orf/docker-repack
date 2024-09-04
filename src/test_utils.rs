use crate::input::layers::InputLayer;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use tar::{Builder, EntryType, Header};

#[derive(Default)]
pub struct LayerBuilder {
    files: Vec<(PathBuf, Vec<u8>)>,
    hardlinks: Vec<(PathBuf, PathBuf)>,
    symlinks: Vec<(PathBuf, PathBuf)>,
    directories: Vec<PathBuf>,
}

impl LayerBuilder {
    pub fn with_files(mut self, files: &[(impl AsRef<Path>, &[u8])]) -> Self {
        self.files
            .extend(files.iter().map(|(p, d)| (p.as_ref().to_path_buf(), d.to_vec())));
        self
    }

    pub fn with_symlinks(mut self, symlinks: &[(impl AsRef<Path>, impl AsRef<Path>)]) -> Self {
        self.symlinks.extend(
            symlinks
                .iter()
                .map(|(p, d)| (p.as_ref().to_path_buf(), d.as_ref().to_path_buf())),
        );
        self
    }

    pub fn with_hardlinks(mut self, hardlinks: &[(impl AsRef<Path>, impl AsRef<Path>)]) -> Self {
        self.hardlinks.extend(
            hardlinks
                .iter()
                .map(|(p, d)| (p.as_ref().to_path_buf(), d.as_ref().to_path_buf())),
        );
        self
    }

    pub fn with_directories(mut self, directories: &[impl AsRef<Path>]) -> Self {
        self.directories
            .extend(directories.iter().map(|p| p.as_ref().to_path_buf()));
        self
    }

    pub fn build(self) -> InputLayer<impl Read> {
        let content = self.build_raw();
        InputLayer::new("test".to_string(), Cursor::new(content)).unwrap()
    }

    pub fn build_raw(self) -> Vec<u8> {
        let mut builder = setup_tar();
        for directory in self.directories {
            add_dir(&mut builder, directory);
        }
        for (path, content) in self.files {
            add_file(&mut builder, path, &content);
        }
        for (path, to_path) in self.hardlinks {
            add_hardlink(&mut builder, path, to_path);
        }
        for (path, to_path) in self.symlinks {
            add_symlink(&mut builder, path, to_path);
        }

        builder.into_inner().unwrap()
    }
}

pub fn read_tar_entries(content: &[u8]) -> Vec<(Header, Vec<u8>)> {
    let mut archive = tar::Archive::new(content);
    archive
        .entries()
        .unwrap()
        .map(|x| {
            let mut entry = x.unwrap();
            let header = entry.header().clone();
            let mut content = vec![];
            entry.read_to_end(&mut content).unwrap();
            (header, content)
        })
        .collect()
}

pub fn read_tar_entries_content(content: &[u8]) -> HashMap<PathBuf, Vec<u8>> {
    let entries = read_tar_entries(content);
    entries
        .into_iter()
        .map(|(header, content)| {
            let path = header.path().unwrap().to_path_buf();
            (path, content)
        })
        .collect()
}

pub fn build_layer() -> LayerBuilder {
    LayerBuilder::default()
}

pub fn setup_tar() -> Builder<Vec<u8>> {
    Builder::new(vec![])
}

pub fn new_header(type_: EntryType, path: impl AsRef<Path>) -> Header {
    let mut header = Header::new_gnu();
    header.set_entry_type(type_);
    header.set_path(path).unwrap();
    header
}

pub fn add_dir(builder: &mut Builder<impl Write>, path: impl AsRef<Path>) {
    let mut header = new_header(EntryType::Directory, path);
    header.set_size(0);
    header.set_cksum();
    builder.append(&header, &mut std::io::empty()).unwrap();
}

pub fn add_file(builder: &mut Builder<impl Write>, path: impl AsRef<Path>, content: &[u8]) {
    let mut header = new_header(EntryType::Regular, &path);
    header.set_size(content.len() as u64);
    builder.append_data(&mut header, &path, content).unwrap();
}

pub fn add_symlink(builder: &mut Builder<impl Write>, path: impl AsRef<Path>, to_path: impl AsRef<Path>) {
    let mut header = new_header(EntryType::Symlink, &path);
    header.set_size(0);
    builder.append_link(&mut header, path, &to_path).unwrap();
}

pub fn add_hardlink(builder: &mut Builder<impl Write>, path: impl AsRef<Path>, to_path: impl AsRef<Path>) {
    let mut header = new_header(EntryType::Link, &path);
    header.set_size(0);
    builder.append_link(&mut header, path, &to_path).unwrap();
}

pub fn compare_paths(paths: Vec<impl AsRef<Path>>, expected: Vec<&str>) {
    let paths: HashSet<_> = paths.iter().map(|v| v.as_ref()).collect();
    let expected: HashSet<_> = expected.iter().map(|v| v.as_ref()).collect();
    assert_eq!(paths, expected);
}
