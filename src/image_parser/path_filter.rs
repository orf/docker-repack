use crate::image_parser::image::LayerID;
use std::collections::HashSet;

pub struct PathFilter<'a> {
    path_set: HashSet<(LayerID, &'a str)>,
}

impl<'a> PathFilter<'a> {
    pub fn from_iter(paths: impl Iterator<Item = (LayerID, &'a str)>) -> Self {
        Self {
            path_set: paths.collect(),
        }
    }
    pub fn contains_path(&self, id: LayerID, path: &str) -> bool {
        self.path_set.contains(&(id, path))
    }
}
