use crate::image_parser::TarItem;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum ItemOperation {
    Add(String),
    RemoveFile(String),
    RemovePrefix(String),
}

impl ItemOperation {
    pub fn from_tar_item(item: &TarItem) -> Self {
        Self::from_path(&item.path)
    }

    fn from_path(path: &Path) -> Self {
        let file_name = path.file_name().unwrap();
        let name_bytes = file_name.as_bytes();
        if file_name == ".wh..wh..opq" {
            let mut prefix = path
                .parent()
                .unwrap()
                .to_path_buf()
                .to_str()
                .unwrap()
                .to_string();
            prefix.push('/');
            Self::RemovePrefix(prefix)
        } else if name_bytes.starts_with(b".wh.") {
            let parent = path.parent().unwrap();
            let name = Path::new(OsStr::from_bytes(&name_bytes[4..]));
            Self::RemoveFile(parent.join(name).to_str().unwrap().to_string())
        } else {
            Self::Add(path.to_str().unwrap().to_string())
        }
    }
}
