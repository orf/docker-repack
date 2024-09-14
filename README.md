# docker-repack

Repack docker images to optimize for pulling speed. 

See the [benchmarks page](https://orf.github.io/docker-repack/benchmarks/) for a full comparison of pulling times across 
many different images.

![](./docs/preview.gif)

## Usage

```bash
$ docker-repack docker://alpine:latest oci://directory/ --target-size=50MB
```

## Installation

### Pre-compiled binaries

Download a release [from the releases page](https://github.com/orf/docker-repack/releases)

### Cargo
```bash
cargo install docker-repack
````

