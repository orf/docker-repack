# Repacking Docker images

Docker repack applies several techniques to optimize Docker images:

- [Removing redundant files](#removing-redundant-data)
- [Compressing duplicate data](#compressing-duplicate-data)
- [Move small files and directories into the first layer](#move-small-files-and-directories-into-the-first-layer)
- [Compressing with zstd](#compressing-with-zstd)

## Removing redundant data

All files and directories that are not present in the final image due to deletions in previous layers or being
overwritten are removed during repacking.

## Compressing duplicate data

All files are hashed when parsing the image. Files that contain duplicate data are stored in the same layer, ensuring
that `zstd` can optimally compress the data to further reduce layer sizes.

## Move small files and directories into the first layer

All "small files" and directories are moved into the first layer of the image. This means that it downloads fastest, 
which allows Docker to begin extracting the huge number of entries within the layer and setting up the filesystem.

## Compressing with zstd

`zstd` is used to compress the layers, which gives a very large reduction in size compared to `gzip`