#!/usr/bin/env bash
set -e

LAYERS="$1"
REGISTRY="$2"
#$(aws ecr get-login-password --region eu-west-2)
USERNAME="$3"
PASSWORD="$4"
MANIFEST="oci/original/blobs/sha256/b721d6003c9613c936670440a8488bfa77ea058dd0b0051694219fe3783dc94a"

trash oci/out/ || true
cargo run --profile=lto -- oci/rootfs oci/out --layers="$LAYERS" --image-config "$MANIFEST"
#skopeo copy oci:oci/out/ docker-daemon:"$REGISTRY":"$LAYERS"-layers
skopeo copy oci:oci/out/ docker://"$REGISTRY":"$LAYERS"-layers --dest-password="$PASSWORD" --dest-username="$USERNAME"
#docker push "$REGISTRY":"$LAYERS"-layers


