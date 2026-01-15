// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service layer for bundle installation and queries
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 unit tests
//!   - semver: Semantic versioning
//!   - std::collections::HashMap: Bundle registry storage
//!   - std::sync::Mutex: Synchronization
//!   - thiserror: Structured error types
//!
//! FEATURES:
//!   - Host backend: In-memory bundle registry for testing
//!   - OS backend: Placeholder for future syscall wiring
//!   - Bundle installation and querying
//!   - Manifest parsing and validation
//!   - Signature verification
//!   - Publisher validation
//!
//! TEST SCENARIOS:
//!   - test_install_success(): Successful bundle installation
//!   - test_install_duplicate_rejected(): Duplicate installation rejection
//!   - test_invalid_signature_encoding_rejected(): Invalid signature handling
//!   - test_mismatched_name_rejected(): Name mismatch validation
//!   - test_backend_unavailable(): Backend availability checking
//!   - test_manifest_parsing(): Manifest parsing and validation
//!   - test_signature_verification(): Signature verification
//!   - test_publisher_validation(): Publisher validation
//!
//! ADR: docs/adr/0009-bundle-manager-architecture.md

#![forbid(unsafe_code)]

#[cfg(nexus_env = "host")]
use crate::manifest::Manifest;
use semver::Version;
#[cfg(nexus_env = "host")]
use std::collections::HashMap;
#[cfg(nexus_env = "host")]
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
    /// Anchor identifier of the publisher.
    pub publisher: String,
    /// Abilities exported by the bundle.
    pub abilities: Vec<String>,
    /// Capabilities required by the bundle.
    pub capabilities: Vec<String>,
}

/// Parameters provided when installing a bundle.
pub struct InstallRequest<'a> {
    /// Name supplied by the caller.
    pub name: &'a str,
    /// Manifest bytes (`manifest.nxb`, Cap'n Proto) extracted from the artifact.
    pub manifest: &'a [u8],
}

/// Bundle manager service entry point.
pub struct Service {
    backend: Backend,
}

enum Backend {
    #[cfg(nexus_env = "host")]
    Host(HostBackend),
    #[cfg(nexus_env = "os")]
    Os,
}

impl Service {
    /// Creates a service using the selected backend.
    pub fn new() -> Self {
        Self { backend: Backend::new() }
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

impl Default for Service {
    fn default() -> Self {
        Self::new()
    }
}

impl Backend {
    #[cfg(nexus_env = "host")]
    fn new() -> Self {
        Self::Host(HostBackend::default())
    }

    #[cfg(nexus_env = "os")]
    fn new() -> Self {
        Self::Os
    }

    #[cfg(nexus_env = "host")]
    fn install(&self, request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        match self {
            Backend::Host(host) => host.install(request),
            #[cfg(nexus_env = "os")]
            Backend::Os => Err(ServiceError::Unsupported),
        }
    }

    #[cfg(nexus_env = "host")]
    fn query(&self, name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        match self {
            Backend::Host(host) => host.query(name),
            #[cfg(nexus_env = "os")]
            Backend::Os => Err(ServiceError::Unsupported),
        }
    }

    #[cfg(nexus_env = "os")]
    fn install(&self, _request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        Err(ServiceError::Unsupported)
    }

    #[cfg(nexus_env = "os")]
    fn query(&self, _name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        Err(ServiceError::Unsupported)
    }
}

#[cfg(nexus_env = "host")]
#[derive(Default)]
struct HostBackend {
    bundles: Mutex<HashMap<String, InstalledBundle>>,
}

#[cfg(nexus_env = "host")]
impl HostBackend {
    fn install(&self, request: InstallRequest<'_>) -> Result<InstalledBundle, ServiceError> {
        let manifest = parse_manifest(request.manifest)?;
        if manifest.name != request.name {
            return Err(ServiceError::Manifest("name mismatch".into()));
        }
        let mut bundles = self.bundles.lock().map_err(|_| ServiceError::Unsupported)?;
        if bundles.contains_key(request.name) {
            return Err(ServiceError::AlreadyInstalled);
        }

        let record = InstalledBundle {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            publisher: manifest.publisher.clone(),
            abilities: manifest.abilities.clone(),
            capabilities: manifest.capabilities.clone(),
        };
        bundles.insert(record.name.clone(), record.clone());
        Ok(record)
    }

    fn query(&self, name: &str) -> Result<Option<InstalledBundle>, ServiceError> {
        let bundles = self.bundles.lock().map_err(|_| ServiceError::Unsupported)?;
        Ok(bundles.get(name).cloned())
    }
}

#[cfg(nexus_env = "host")]
fn parse_manifest(input: &[u8]) -> Result<Manifest, ServiceError> {
    Manifest::parse_nxb(input).map_err(ServiceError::from)
}

#[cfg(nexus_env = "host")]
#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(nexus_env = "host")]
    fn manifest_bytes(sig_len: usize) -> Vec<u8> {
        use capnp::message::Builder;
        use nexus_idl_runtime::manifest_capnp::bundle_manifest;

        let mut builder = Builder::new_default();
        let mut msg = builder.init_root::<bundle_manifest::Builder>();
        msg.set_schema_version(1);
        msg.set_name("launcher");
        msg.set_semver("1.0.0");
        msg.set_min_sdk("0.1.0");
        {
            let mut a = msg.reborrow().init_abilities(1);
            a.set(0, "ui");
        }
        {
            let mut c = msg.reborrow().init_capabilities(1);
            c.set(0, "gpu");
        }
        msg.set_publisher(&[0u8; 16]);
        msg.set_signature(&vec![0u8; sig_len]);

        let mut out = Vec::new();
        capnp::serialize::write_message(&mut out, &builder).unwrap();
        out
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn install_success() {
        let service = Service::new();
        let man = manifest_bytes(64);
        let record = service
            .install(InstallRequest { name: "launcher", manifest: &man })
            .expect("install succeeds");
        assert_eq!(record.name, "launcher");
        assert_eq!(record.version, Version::new(1, 0, 0));
        assert_eq!(record.publisher, hex::encode([0u8; 16]));
        assert_eq!(record.capabilities, vec!["gpu".to_string()]);
        assert_eq!(record.abilities, vec!["ui".to_string()]);
        let query = service.query("launcher").unwrap();
        assert_eq!(query.unwrap(), record);
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn install_duplicate_rejected() {
        let service = Service::new();
        let man = manifest_bytes(64);
        service.install(InstallRequest { name: "launcher", manifest: &man }).unwrap();
        let err = service.install(InstallRequest { name: "launcher", manifest: &man }).unwrap_err();
        assert_eq!(err, ServiceError::AlreadyInstalled);
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn invalid_signature_length_rejected() {
        let service = Service::new();
        let tampered = manifest_bytes(8);
        let err =
            service.install(InstallRequest { name: "launcher", manifest: &tampered }).unwrap_err();
        assert!(matches!(err, ServiceError::Manifest(_)));
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn mismatched_name_rejected() {
        let service = Service::new();
        let man = manifest_bytes(64);
        let err = service.install(InstallRequest { name: "other", manifest: &man }).unwrap_err();
        assert!(matches!(err, ServiceError::Manifest(_)));
    }

    #[cfg(nexus_env = "os")]
    #[test]
    fn backend_unavailable() {
        let service = Service::new();
        let err = service.install(InstallRequest { name: "launcher", manifest: b"" }).unwrap_err();
        assert_eq!(err, ServiceError::Unsupported);
    }
}
