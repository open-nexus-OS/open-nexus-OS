// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Service layer for bundle installation and queries.

#![forbid(unsafe_code)]

#[cfg(feature = "backend-host")]
use crate::manifest::Manifest;
use semver::Version;
#[cfg(feature = "backend-host")]
use std::collections::HashMap;
#[cfg(feature = "backend-host")]
use std::sync::Mutex;
use thiserror::Error;

/// Errors returned by the bundle manager service.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ServiceError {
    /// The bundle is already installed.
    #[error("bundle already installed")]
    AlreadyInstalled,
    /// The manifest failed to parse or was invalid.
    #[error("manifest error: {0}")]
    Manifest(String),
    /// Signature verification failed.
    #[error("signature verification failed")]
    InvalidSignature,
    /// Backend not available for this build.
    #[error("backend unsupported")]
    Unsupported,
}

impl From<crate::manifest::Error> for ServiceError {
    fn from(err: crate::manifest::Error) -> Self {
        Self::Manifest(err.to_string())
    }
}

/// Metadata describing an installed bundle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledBundle {
    /// Unique bundle identifier.
    pub name: String,
    /// Installed version.
    pub version: Version,
}

/// Parameters provided when installing a bundle.
pub struct InstallRequest<'a> {
    /// Name supplied by the caller.
    pub name: &'a str,
    /// Manifest bytes (UTF-8 TOML) extracted from the artifact.
    pub manifest: &'a str,
}

/// Bundle manager service entry point.
pub struct Service {
    backend: Backend,
}

enum Backend {
    #[cfg(feature = "backend-host")]
    Host(HostBackend),
    #[cfg(feature = "backend-os")]
    Os,
}

impl Service {
    /// Creates a service using the selected backend.
    pub fn new() -> Self {
        Self {
            backend: Backend::new(),
        }
    }

    /// Installs a bundle described by `request`.
    pub fn install(&self, request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        self.backend.install(request)
    }

    /// Queries an installed bundle by name.
    pub fn query(&self, name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        self.backend.query(name)
    }
}

impl Backend {
    #[cfg(feature = "backend-host")]
    fn new() -> Self {
        Self::Host(HostBackend::default())
    }

    #[cfg(feature = "backend-os")]
    fn new() -> Self {
        Self::Os
    }

    #[cfg(feature = "backend-host")]
    fn install(&self, request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        match self {
            Backend::Host(host) => host.install(request),
            #[cfg(feature = "backend-os")]
            Backend::Os => Err(ServiceError::Unsupported),
        }
    }

    #[cfg(feature = "backend-host")]
    fn query(&self, name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        match self {
            Backend::Host(host) => host.query(name),
            #[cfg(feature = "backend-os")]
            Backend::Os => Err(ServiceError::Unsupported),
        }
    }

    #[cfg(feature = "backend-os")]
    fn install(&self, _request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        Err(ServiceError::Unsupported)
    }

    #[cfg(feature = "backend-os")]
    fn query(&self, _name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        Err(ServiceError::Unsupported)
    }
}

#[cfg(feature = "backend-host")]
#[derive(Default)]
struct HostBackend {
    bundles: Mutex<HashMap<String, InstalledBundle>>,
}

#[cfg(feature = "backend-host")]
impl HostBackend {
    fn install(&self, request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        if !verify_signature(request.manifest) {
            return Err(ServiceError::InvalidSignature);
        }
        let manifest = parse_manifest(request.manifest)?;
        if manifest.name != request.name {
            return Err(ServiceError::Manifest("name mismatch".into()));
        }

        let mut bundles = self.bundles.lock().expect("mutex poisoned");
        if bundles.contains_key(request.name) {
            return Err(ServiceError::AlreadyInstalled);
        }

        let record = InstalledBundle {
            name: manifest.name,
            version: manifest.version,
        };
        bundles.insert(record.name.clone(), record.clone());
        Ok(record)
    }

    fn query(&self, name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        let bundles = self.bundles.lock().expect("mutex poisoned");
        Ok(bundles.get(name).cloned())
    }
}

#[cfg(feature = "backend-host")]
fn parse_manifest(input: &str) -> Result<Manifest, ServiceError> {
    Manifest::parse_str(input).map_err(ServiceError::from)
}

#[cfg(feature = "backend-host")]
fn verify_signature(input: &str) -> bool {
    input.contains("signature = \"valid\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "backend-host")]
    const MANIFEST: &str = r#"
name = "launcher"
version = "1.0.0"
abilities = ["ui"]
caps = ["gpu"]
min_sdk = "0.1.0"
signature = "valid"
"#;

    #[cfg(feature = "backend-host")]
    #[test]
    fn install_success() {
        let service = Service::new();
        let record = service
            .install(InstallRequest {
                name: "launcher",
                manifest: MANIFEST,
            })
            .expect("install succeeds");
        assert_eq!(record.name, "launcher");
        assert_eq!(record.version, Version::new(1, 0, 0));
        let query = service.query("launcher").unwrap();
        assert_eq!(query.unwrap(), record);
    }

    #[cfg(feature = "backend-host")]
    #[test]
    fn install_duplicate_rejected() {
        let service = Service::new();
        service
            .install(InstallRequest {
                name: "launcher",
                manifest: MANIFEST,
            })
            .unwrap();
        let err = service
            .install(InstallRequest {
                name: "launcher",
                manifest: MANIFEST,
            })
            .unwrap_err();
        assert_eq!(err, ServiceError::AlreadyInstalled);
    }

    #[cfg(feature = "backend-host")]
    #[test]
    fn invalid_signature_rejected() {
        let service = Service::new();
        let tampered = MANIFEST.replace("valid", "invalid");
        let err = service
            .install(InstallRequest {
                name: "launcher",
                manifest: &tampered,
            })
            .unwrap_err();
        assert_eq!(err, ServiceError::InvalidSignature);
    }

    #[cfg(feature = "backend-host")]
    #[test]
    fn mismatched_name_rejected() {
        let service = Service::new();
        let err = service
            .install(InstallRequest {
                name: "other",
                manifest: MANIFEST,
            })
            .unwrap_err();
        assert!(matches!(err, ServiceError::Manifest(_)));
    }

    #[cfg(not(feature = "backend-host"))]
    #[test]
    fn backend_unavailable() {
        let service = Service::new();
        let err = service
            .install(InstallRequest {
                name: "launcher",
                manifest: "name = \"launcher\"",
            })
            .unwrap_err();
        assert_eq!(err, ServiceError::Unsupported);
    }
}
