use oci_client::Reference;
use std::fmt::Display;
use std::path::PathBuf;
use std::str::FromStr;

#[derive(Debug, Clone)]
pub enum Location {
    Oci(PathBuf),
    Docker(Reference),
}

impl Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Location::Oci(path) => write!(f, "oci://{}", path.display()),
            Location::Docker(reference) => write!(f, "docker://{}", reference),
        }
    }
}

impl FromStr for Location {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.split_once("://") {
            None => Ok(Self::Docker(value.parse()?)),
            Some(("oci", path)) => Ok(Self::Oci(path.into())),
            Some(("docker", reference)) => Ok(Self::Docker(reference.parse()?)),
            Some((prefix, _)) => Err(anyhow::anyhow!("Invalid image type {prefix}")),
        }
    }
}
