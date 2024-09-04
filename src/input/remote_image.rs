use crate::input::layers::InputLayer;
use crate::input::{get_layer_media_type, InputImage};
use crate::utils;
use anyhow::Context;
use docker_credential::{CredentialRetrievalError, DockerCredential};
use itertools::Itertools;
use oci_client::manifest::{OciImageManifest, OciManifest, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use oci_spec::image::{ImageConfiguration, MediaType};
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::io::Read;
use tokio::io::BufReader;
use tokio::runtime::Handle;
use tokio_util::io::SyncIoBridge;
use tracing::{debug, instrument, trace, warn};

fn build_auth(reference: &Reference) -> anyhow::Result<RegistryAuth> {
    let server = reference
        .resolve_registry()
        .strip_suffix('/')
        .unwrap_or_else(|| reference.resolve_registry());

    match docker_credential::get_credential(server) {
        Err(CredentialRetrievalError::ConfigNotFound) => Ok(RegistryAuth::Anonymous),
        Err(CredentialRetrievalError::NoCredentialConfigured) => Ok(RegistryAuth::Anonymous),
        Err(e) => {
            match e {
                CredentialRetrievalError::HelperFailure { stdout, stderr } => {
                    let base_message =
                        "Credential helper returned non-zero response code, falling back to anonymous auth";
                    if !stderr.is_empty() || !stdout.is_empty() {
                        let extra = [stdout.trim(), stderr.trim()].join(" - ");
                        warn!("{base_message}: stdout/stderr = {extra}");
                    } else {
                        warn!("{base_message}");
                    };
                }
                _ => {
                    warn!("Error getting docker credentials, falling back to anonymous auth: {e}");
                }
            }

            Ok(RegistryAuth::Anonymous)
        }
        Ok(DockerCredential::UsernamePassword(username, password)) => {
            debug!("Found docker credentials");
            Ok(RegistryAuth::Basic(username, password))
        }
        Ok(DockerCredential::IdentityToken(_)) => {
            warn!("Cannot use contents of docker config, identity token not supported. Using anonymous auth");
            Ok(RegistryAuth::Anonymous)
        }
    }
}

pub struct RemoteImage {
    client: Client,
    reference: Reference,
    layers: Vec<(MediaType, String)>,
    config_digest: String,
    image_config: ImageConfiguration,
    handle: Handle,
}

impl PartialEq for RemoteImage {
    fn eq(&self, other: &Self) -> bool {
        self.image_digest() == other.image_digest()
    }
}

impl Eq for RemoteImage {}

impl Hash for RemoteImage {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.image_digest().hash(state);
    }
}

impl Debug for RemoteImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteImage")
            .field("reference", &self.reference)
            .field("layers", &self.layers)
            .field("image_config", &self.image_config)
            .finish()
    }
}

impl Display for RemoteImage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "{} {} {}",
            self.reference,
            self.image_config.os(),
            self.image_config.architecture()
        ))
    }
}

impl RemoteImage {
    // #[instrument(skip_all, fields(image = %reference))]
    #[instrument(name = "load_images", skip_all, fields(image = %reference))]
    pub fn create_remote_images(handle: &Handle, reference: Reference) -> anyhow::Result<Vec<Self>> {
        handle.block_on(Self::from_list_async(reference))
    }

    async fn from_list_async(reference: Reference) -> anyhow::Result<Vec<Self>> {
        let auth = build_auth(&reference).context("Get Authentication")?;
        let client = Client::new(Default::default());
        debug!("Fetching manifest list for {}", reference);
        let (manifest_content, _) = client
            .pull_manifest(&reference, &auth)
            .await
            .context("Fetch manifest list")?;
        match manifest_content {
            OciManifest::Image(image) => {
                debug!("Found single image manifest");
                let img = Self::from_image_manifest(reference, image, client).await?;
                Ok(vec![img])
            }
            OciManifest::ImageIndex(index) => {
                debug!("Found image index");
                let iterator = utils::progress_iter("Reading Manifests", index.manifests.into_iter());
                let manifests = iterator
                    .filter(|entry| match entry.platform.as_ref() {
                        Some(platform) if platform.os == "linux" => true,
                        _ => {
                            trace!("Skipping unknown platform manifest for entry: {:?}", entry);
                            false
                        }
                    })
                    .filter_map(|entry| {
                        let media_type = entry.media_type.as_str();
                        trace!("Checking entry media type ({media_type}) {:?}", entry);
                        match media_type {
                            OCI_IMAGE_MEDIA_TYPE | IMAGE_MANIFEST_MEDIA_TYPE => {
                                trace!("Found image manifest");
                                Some(reference.clone_with_digest(entry.digest))
                            }
                            _ => {
                                trace!("Skipped");
                                None
                            }
                        }
                    });
                let mut images = vec![];
                for manifest in manifests {
                    {
                        let img = Self::from_image_reference(manifest, client.clone(), auth.clone()).await?;
                        images.push(img);
                        // Super hacky, but we need to sleep here to avoid rate limiting.
                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                    }
                }
                debug!("Found {} images", images.len());
                Ok(images)
            }
        }
    }

    // #[instrument(skip_all, fields(image = %reference))]
    async fn from_image_reference(reference: Reference, client: Client, auth: RegistryAuth) -> anyhow::Result<Self> {
        debug!("Fetching manifest for {}", reference);
        let (manifest_content, _) = client
            .pull_manifest_raw(&reference, &auth, &[OCI_IMAGE_MEDIA_TYPE])
            .await
            .with_context(|| format!("Fetching manifest {reference}"))?;
        let manifest: OciImageManifest = serde_json::from_slice(&manifest_content).context("Parse ImageManifest")?;
        trace!("Manifest parsed for {}: {:#?}", reference, manifest);
        Self::from_image_manifest(reference, manifest, client)
            .await
            .context("from_image_manifest")
    }

    async fn from_image_manifest(
        reference: Reference,
        manifest: OciImageManifest,
        client: Client,
    ) -> anyhow::Result<Self> {
        let mut config_data = vec![];
        let config_digest = manifest
            .config
            .digest
            .strip_prefix("sha256:")
            .unwrap_or(&manifest.config.digest)
            .to_string();
        debug!("Fetching config for {}", config_digest);
        client
            .pull_blob(&reference, &manifest.config, &mut config_data)
            .await
            .with_context(|| format!("Fetch config {}", manifest.config))?;
        let image_config = ImageConfiguration::from_reader(&config_data[..]).context("Parse ImageConfiguration")?;

        let layers = manifest
            .layers
            .into_iter()
            .filter_map(|v| {
                let media_type = v.media_type.as_str();
                if let Some(parsed_media_type) = get_layer_media_type(media_type) {
                    trace!("Found layer descriptor: {:?}", v);
                    Some((parsed_media_type, v.digest))
                } else {
                    trace!("Skipping descriptor: {:?}", v);
                    None
                }
            })
            .collect_vec();
        let handle = Handle::current();
        Ok(Self {
            client,
            reference,
            layers,
            image_config,
            handle,
            config_digest,
        })
    }
}

impl InputImage for RemoteImage {
    fn image_digest(&self) -> String {
        self.config_digest.clone()
    }

    fn layers_from_manifest(
        &self,
    ) -> anyhow::Result<impl ExactSizeIterator<Item = anyhow::Result<InputLayer<impl Read>>>> {
        Ok(self.layers_with_compression()?.map(|(compression, digest)| {
            debug!("Fetching blob stream for {}", digest);
            let res = self
                .handle
                .block_on(self.client.pull_blob_stream(&self.reference, digest.as_str()))?;

            let reader = tokio_util::io::StreamReader::new(res);
            let reader = BufReader::with_capacity(5 * 1024 * 1024, reader);
            let bridge = SyncIoBridge::new_with_handle(reader, self.handle.clone());
            let reader = compression.new_reader(bridge)?;
            InputLayer::new(digest, reader)
        }))
    }

    fn config(&self) -> &ImageConfiguration {
        &self.image_config
    }

    fn layers(&self) -> anyhow::Result<Vec<(MediaType, String)>> {
        Ok(self.layers.clone())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test_log::test]
    fn test_remote_image() {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let reference = "alpine:3.20".parse().unwrap();
        let images = RemoteImage::create_remote_images(runtime.handle(), reference).unwrap();
        assert_ne!(images.len(), 0);
        for image in images {
            let layers = image.layers().unwrap();
            assert_ne!(layers, vec![], "No layers found");

            let compression = image.layers_with_compression().unwrap().collect_vec();
            assert_ne!(compression, vec![], "No compression found");

            let mut count = 0;
            for layer in image.layers_from_manifest().unwrap() {
                let mut input_layer = layer.unwrap();
                let entries = input_layer.entries().unwrap().count();
                assert_ne!(entries, 0);
                count += 1;
            }
            assert_eq!(count, layers.len());
            assert_eq!(count, compression.len());
        }
    }
}
