#!/usr/bin/env bash
set -e

NAME="$1"
IMAGE="$2"
#IMAGE="python:3.11"
#docker pull "$IMAGE"

#docker export "$(docker create "$IMAGE")" --output="oci/rootfs.tar"
#skopeo copy --override-arch=arm64 --override-os=linux docker://"$IMAGE" oci:oci/original/

mkdir -p oci/"$NAME"/image

echo "FROM $IMAGE" | docker buildx build - --platform=linux/arm64 --pull --builder=container -o type=oci,tar=false,dest=oci/"$NAME"/image/
echo "FROM $IMAGE" | docker buildx build - --platform=linux/arm64 --pull --builder=container -o type=tar | gzip -1 > oci/"$NAME"/rootfs.tar.gz -
