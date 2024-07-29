use crate::content;
use crate::content::merged::MergedLayerContent;
use crate::io::layer::reader::DecompressedLayer;
use crate::tar_item::TarItem;
use crate::utils::display_bytes;
use globset::GlobSet;
use indicatif::MultiProgress;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tar::Archive;
use tracing::info;

pub fn get_layer_contents(
    progress: &MultiProgress,
    layers: &[DecompressedLayer],
    exclude: Option<GlobSet>,
) -> anyhow::Result<MergedLayerContent> {
    let all_operations: Result<Vec<_>, anyhow::Error> = layers
        .into_par_iter()
        .map(|layer| {
            let reader = layer.get_progress_reader(progress, "Reading layer")?;
            let mut archive = Archive::new(reader);
            let items = archive
                .entries_with_seek()
                .unwrap()
                .flatten()
                .map(|mut entry| TarItem::from_entry(layer.id, &mut entry).unwrap());

            content::operations::LayerOperations::from_tar_items(items)
        })
        .collect();

    let mut layer_contents = MergedLayerContent::default();
    for operations in all_operations? {
        layer_contents = layer_contents.merge_operations(operations);
    }

    if let Some(glob_set) = exclude {
        let (excluded_count, excluded_size) = layer_contents.exclude_globs(glob_set);
        info!(
            "Excluded {} items ({:#.1})",
            excluded_count,
            display_bytes(excluded_size)
        );
    }
    Ok(layer_contents)
}
