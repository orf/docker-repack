use globset::{Glob, GlobBuilder, GlobMatcher};
use oci_client::manifest::Platform as OciClientPlatform;
use oci_spec::image::Platform;
use std::fmt::{Display, Formatter};
use tracing::{debug, instrument};

#[derive(Debug)]
pub struct PlatformMatcher {
    glob: GlobMatcher,
    exclude: GlobMatcher,
}

impl Display for PlatformMatcher {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("PlatformMatcher: ")?;
        f.write_str(self.glob.glob().glob())?;
        f.write_str(" (exclude: ")?;
        f.write_str(self.exclude.glob().glob())?;
        f.write_str(")")
    }
}

impl PlatformMatcher {
    pub fn from_glob(glob: Glob) -> anyhow::Result<Self> {
        let glob_pattern = glob.glob();
        debug!("Creating platform matcher for glob pattern: {}", glob_pattern);

        let glob = GlobBuilder::new(glob_pattern)
            .case_insensitive(true)
            .literal_separator(false)
            .build()?;
        let exclude = GlobBuilder::new("unknown/*")
            .case_insensitive(true)
            .literal_separator(false)
            .build()?;

        let exclude = exclude.compile_matcher();
        let glob = glob.compile_matcher();
        Ok(Self { glob, exclude })
    }

    #[cfg(test)]
    pub fn match_all() -> Self {
        let glob = Glob::new("*").unwrap();
        Self::from_glob(glob).unwrap()
    }

    #[instrument(level = "debug")]
    pub fn matches_str(&self, os: &str, arch: &str, variant: &str) -> bool {
        let os = os.to_ascii_lowercase();
        let arch = arch.to_ascii_lowercase();
        let variant = variant.to_ascii_lowercase();

        let os_arch = format!("{}/{}", os, arch);
        let os_arch_variant = format!("{}/{}/{}", os, arch, variant);

        if self.exclude.is_match(&os_arch_variant) || self.exclude.is_match(&os_arch) {
            debug!("Platform is excluded");
            return false;
        }

        let result = self.glob.is_match(os_arch) || self.glob.is_match(&os_arch_variant);
        if result {
            debug!("Platform matched");
        } else {
            debug!("Platform does not match");
        }
        result
    }

    pub fn matches_oci_spec_platform(&self, platform: Option<&Platform>) -> bool {
        match platform {
            Some(platform) => {
                let os = platform.os();
                let arch = platform.architecture();
                let variant = platform.variant().as_ref().map(|s| s.as_str()).unwrap_or("unknown");
                self.matches_str(&os.to_string(), &arch.to_string(), variant)
            }
            None => {
                // If no platform is specified, we assume it matches
                true
            }
        }
    }

    pub fn matches_oci_client_platform(&self, platform: Option<&OciClientPlatform>) -> bool {
        match platform {
            Some(platform) => {
                let os = &platform.os;
                let arch = &platform.architecture;
                let variant = platform.variant.as_deref().unwrap_or("unknown");
                self.matches_str(os, arch, variant)
            }
            None => {
                // If no platform is specified, we assume it matches
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oci_spec::image::PlatformBuilder;
    #[test]
    fn test_matcher() {
        for pattern in ["linux/*", "linux/amd64"] {
            let matcher = PlatformMatcher::from_glob(Glob::new(pattern).unwrap()).unwrap();
            assert!(matcher.matches_str("linux", "amd64", "unknown"));
            assert!(!matcher.matches_str("windows", "amd64", "unknown"));
            assert!(!matcher.matches_str("unknown", "unknown", "unknown"));
        }
    }

    #[test]
    fn test_matcher_insensitive() {
        let matcher = PlatformMatcher::from_glob(Glob::new("linux/*").unwrap()).unwrap();
        assert!(matcher.matches_str("Linux", "amd64", "unknown"));
        let matcher = PlatformMatcher::from_glob(Glob::new("linux/amd64").unwrap()).unwrap();
        assert!(matcher.matches_str("Linux", "Amd64", "unknown"));
    }

    #[test]
    fn test_oci_client_matcher() {
        let platform = OciClientPlatform {
            os: "Linux".to_string(),
            architecture: "arm64".to_string(),
            os_version: None,
            os_features: None,
            variant: Some("v8".to_string()),
            features: None,
        };

        let platform_unknown = OciClientPlatform {
            os: "Unknown".to_string(),
            architecture: "Unknown".to_string(),
            os_version: None,
            os_features: None,
            variant: Some("Unknown".to_string()),
            features: None,
        };

        for pattern in ["linux/*", "linux/arm64", "linux/arm64/v8", "*/arm64/v8"] {
            let matcher = PlatformMatcher::from_glob(Glob::new(pattern).unwrap()).unwrap();
            assert!(matcher.matches_oci_client_platform(Some(&platform)));
            assert!(!matcher.matches_oci_client_platform(Some(&platform_unknown)));
            assert!(matcher.matches_oci_client_platform(None));
        }
    }

    #[test]
    fn test_oci_spec_platform() {
        let platform = PlatformBuilder::default()
            .os("linux")
            .architecture("arm64")
            .variant("v8")
            .build()
            .unwrap();
        let platform_unknown = PlatformBuilder::default()
            .os("unknown")
            .architecture("unknown")
            .variant("unknown")
            .build()
            .unwrap();

        for pattern in ["linux/*", "linux/arm64", "linux/arm64/v8", "*/arm64/v8"] {
            let matcher = PlatformMatcher::from_glob(Glob::new(pattern).unwrap()).unwrap();
            assert!(matcher.matches_oci_spec_platform(Some(&platform)));
            assert!(!matcher.matches_oci_spec_platform(Some(&platform_unknown)));
            assert!(matcher.matches_oci_spec_platform(None));
        }
    }
}
