#!/usr/bin/env bash


LAYERS="$1"
REGISTRY="277105304060.dkr.ecr.eu-west-2.amazonaws.com/python-test"
trash oci/out/ || true
cargo run --release -- oci/rootfs oci/out --layers="$LAYERS" --image-config oci/original/blobs/sha256/b721d6003c9613c936670440a8488bfa77ea058dd0b0051694219fe3783dc94a
skopeo copy oci:oci/out/ docker-daemon:"$REGISTRY":"$LAYERS"-layers
docker push "$REGISTRY":"$LAYERS"-layers
