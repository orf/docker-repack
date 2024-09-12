use crate::input::layers::InputLayer;
use crate::input::{get_layer_media_type, InputImage};
use crate::platform_matcher::PlatformMatcher;
use crate::progress;
use anyhow::Context;
use docker_credential::{CredentialRetrievalError, DockerCredential};
use itertools::Itertools;
use oci_client::manifest::{OciImageManifest, OciManifest, IMAGE_MANIFEST_MEDIA_TYPE, OCI_IMAGE_MEDIA_TYPE};
use oci_client::secrets::RegistryAuth;
use oci_client::{Client, Reference};
use oci_spec::image::{Digest, ImageConfiguration, MediaType};
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::str::FromStr;
use tokio::io::BufReader;
use tokio::runtime::Handle;
use tokio_util::io::SyncIoBridge;
use tracing::{debug, instrument, trace, warn};

#[instrument(skip_all, fields(reference = %reference))]
fn build_auth(reference: &Reference) -> RegistryAuth {
    let server = reference
        .resolve_registry()
        .strip_suffix('/')
        .unwrap_or_else(|| reference.resolve_registry());

    let auth_results = [
        ("docker", docker_credential::get_credential(server)),
        ("podman", docker_credential::get_podman_credential(server)),
    ];

    for (name, cred_result) in auth_results.into_iter() {
        match cred_result {
            Err(e) => match e {
                CredentialRetrievalError::HelperFailure { stdout, stderr } => {
                    let base_message =
                        "Credential helper returned non-zero response code, falling back to anonymous auth";
                    if !stderr.is_empty() || !stdout.is_empty() {
                        let extra = [stdout.trim(), stderr.trim()].join(" - ");
                        warn!("{name}: {base_message}: stdout/stderr = {extra}");
                    } else {
                        warn!("{name}: {base_message}");
                    };
                }
                e => {
                    debug!("{name}: {e}");
                }
            },
            Ok(DockerCredential::UsernamePassword(username, password)) => {
                debug!("{name}: Found docker credentials");
                return RegistryAuth::Basic(username, password);
            }
            Ok(DockerCredential::IdentityToken(_)) => {
                warn!("{name}: Cannot use contents of docker config, identity token not supported.");
            }
        };
    }
    debug!("No credentials found, using anonymous auth");
    RegistryAuth::Anonymous
}

pub struct RemoteImage {
    client: Client,
    reference: Reference,
    layers: Vec<(MediaType, Digest)>,
    config_digest: Digest,
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
        let digest = self.image_digest();
        digest.digest().hash(state);
        digest.algorithm().as_ref().hash(state);
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
    #[instrument(name = "load_images", skip_all, fields(image = %reference))]
    pub fn create_remote_images(
        handle: &Handle,
        reference: Reference,
        platform: &PlatformMatcher,
    ) -> anyhow::Result<Vec<Self>> {
        handle.block_on(Self::from_list_async(reference, platform))
    }

    async fn from_list_async(reference: Reference, platform_matcher: &PlatformMatcher) -> anyhow::Result<Vec<Self>> {
        let auth = build_auth(&reference);
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
                let iterator = progress::progress_iter("Reading Manifests", index.manifests.into_iter());
                let manifests = iterator
                    .filter(|entry| platform_matcher.matches_oci_client_platform(entry.platform.as_ref()))
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
        let config_digest = Digest::from_str(&manifest.config.digest)?;
        debug!("Fetching config for {}", config_digest);
        client
            .pull_blob(&reference, &manifest.config, &mut config_data)
            .await
            .with_context(|| format!("Fetch config {}", manifest.config))?;
        let image_config = ImageConfiguration::from_reader(&config_data[..]).context("Parse ImageConfiguration")?;

        let layers = manifest
            .layers
            .into_iter()
            .map(|v| {
                let media_type = v.media_type.as_str();
                if let Some(parsed_media_type) = get_layer_media_type(media_type) {
                    trace!("Found layer descriptor: {:?}", v);
                    let digest = Digest::from_str(&v.digest)?;
                    Ok(Some((parsed_media_type, digest)))
                } else {
                    trace!("Skipping descriptor: {:?}", v);
                    Ok(None)
                }
            })
            .filter_map_ok(|r| r)
            .collect::<anyhow::Result<Vec<_>>>();
        let handle = Handle::current();
        Ok(Self {
            client,
            reference,
            layers: layers?,
            image_config,
            handle,
            config_digest,
        })
    }
}

impl InputImage for RemoteImage {
    fn image_digest(&self) -> Digest {
        self.config_digest.clone()
    }

    fn layers_from_manifest(
        &self,
    ) -> anyhow::Result<impl ExactSizeIterator<Item = anyhow::Result<InputLayer<impl Read>>>> {
        Ok(self.layers_with_compression()?.map(|(compression, digest)| {
            debug!("Fetching blob stream for {}", digest);
            let res = self.handle.block_on(
                self.client
                    .pull_blob_stream(&self.reference, digest.to_string().as_str()),
            )?;

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

    fn layers(&self) -> anyhow::Result<Vec<(MediaType, Digest)>> {
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
        let matcher = crate::platform_matcher::PlatformMatcher::match_all();
        let images = RemoteImage::create_remote_images(runtime.handle(), reference, &matcher).unwrap();
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
