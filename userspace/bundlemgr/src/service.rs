// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Service layer for bundle installation and queries
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 4 unit tests (module tests)
//!   - install_success(): happy-path install + query
//!   - install_duplicate_rejected(): duplicate rejection
//!   - invalid_signature_length_rejected(): malformed signature
//!   - mismatched_name_rejected(): name mismatch
//!
//! DEPENDENCIES:
//!   - semver: Semantic versioning
//!   - std::collections::HashMap: Bundle registry storage
//!   - std::sync::Mutex: Synchronization
//!   - thiserror: Structured error types
//!
//! FEATURES:
//!   - Host backend: In-memory bundle registry for testing
//!   - OS backend: Placeholder for future syscall wiring
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

/// Launcher/SystemUI-facing projection of an installed bundle — the "which apps
/// exist" record the app registry hands out via `enumerate`. Derived from
/// [`InstalledBundle`]; it carries only fields the manifest actually provides
/// today, so nothing is fabricated (see RFC-0065 §App registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppRecord {
    /// Stable bundle/app id (the bundle name, e.g. `"search"`).
    pub id: String,
    /// Human label for the launcher/recents. No manifest label field exists yet,
    /// so this falls back to `id` until manifests carry a display name.
    pub display_name: String,
    /// Ability entrypoint `abilitymgr` launches (the bundle's first declared
    /// ability). Empty when the bundle declares no abilities.
    pub launch_ability: String,
    /// Capabilities the bundle requires (carried straight from the manifest).
    pub required_caps: Vec<String>,
}

impl InstalledBundle {
    /// Projects this installed bundle into the registry-facing [`AppRecord`].
    pub fn to_app_record(&self) -> AppRecord {
        AppRecord {
            id: self.name.clone(),
            display_name: self.name.clone(),
            launch_ability: self.abilities.first().cloned().unwrap_or_default(),
            required_caps: self.capabilities.clone(),
        }
    }
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

    /// Enumerates all installed bundles, sorted by name for deterministic output.
    ///
    /// This is the registry-wide listing the app launcher/SystemUI build on — the
    /// single source of "which apps exist" (RFC-0065). Clients enumerate it instead
    /// of hardcoding an app list.
    pub fn enumerate(&self) -> Result<Vec<InstalledBundle>, ServiceError> {
        self.backend.enumerate()
    }

    /// Enumerates all installed apps as registry-facing [`AppRecord`]s (sorted by id).
    pub fn enumerate_apps(&self) -> Result<Vec<AppRecord>, ServiceError> {
        Ok(self.enumerate()?.iter().map(InstalledBundle::to_app_record).collect())
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

    #[cfg(nexus_env = "host")]
    fn enumerate(&self) -> Result<Vec<InstalledBundle>, ServiceError> {
        match self {
            Backend::Host(host) => host.enumerate(),
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

    #[cfg(nexus_env = "os")]
    fn enumerate(&self) -> Result<Vec<InstalledBundle>, ServiceError> {
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

    fn enumerate(&self) -> Result<Vec<InstalledBundle>, ServiceError> {
        let bundles = self.bundles.lock().map_err(|_| ServiceError::Unsupported)?;
        let mut out: Vec<InstalledBundle> = bundles.values().cloned().collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
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
        manifest_named("launcher", "ui", "gpu", sig_len)
    }

    #[cfg(nexus_env = "host")]
    fn manifest_named(name: &str, ability: &str, cap: &str, sig_len: usize) -> Vec<u8> {
        use capnp::message::Builder;
        use nexus_idl_runtime::manifest_capnp::bundle_manifest;

        let mut builder = Builder::new_default();
        let mut msg = builder.init_root::<bundle_manifest::Builder>();
        msg.set_schema_version(1);
        msg.set_name(name);
        msg.set_semver("1.0.0");
        msg.set_min_sdk("0.1.0");
        {
            let mut a = msg.reborrow().init_abilities(1);
            a.set(0, ability);
        }
        {
            let mut c = msg.reborrow().init_capabilities(1);
            c.set(0, cap);
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

    #[cfg(nexus_env = "host")]
    #[test]
    fn enumerate_empty_registry() {
        let service = Service::new();
        let apps = service.enumerate().expect("enumerate succeeds");
        assert!(apps.is_empty(), "fresh registry enumerates to nothing");
        assert!(service.enumerate_apps().unwrap().is_empty());
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn enumerate_is_sorted_and_complete() {
        let service = Service::new();
        // Install out of order; enumerate must return them name-sorted (deterministic).
        for name in ["search", "chat", "notes"] {
            let man = manifest_named(name, "ui", "gpu", 64);
            service.install(InstallRequest { name, manifest: &man }).expect("install");
        }
        let names: Vec<String> = service.enumerate().unwrap().into_iter().map(|b| b.name).collect();
        assert_eq!(names, vec!["chat", "notes", "search"]);
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn enumerate_apps_projects_app_records() {
        let service = Service::new();
        let man = manifest_named("search", "search.main", "gpu", 64);
        service.install(InstallRequest { name: "search", manifest: &man }).expect("install");

        let apps = service.enumerate_apps().expect("enumerate apps");
        assert_eq!(apps.len(), 1);
        let rec = &apps[0];
        assert_eq!(rec.id, "search");
        assert_eq!(rec.display_name, "search"); // falls back to id (no manifest label yet)
        assert_eq!(rec.launch_ability, "search.main"); // first declared ability
        assert_eq!(rec.required_caps, vec!["gpu".to_string()]);
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn to_app_record_handles_no_abilities() {
        // A bundle with no abilities projects to an empty launch_ability, not a panic.
        let bundle = InstalledBundle {
            name: "headless".into(),
            version: Version::new(1, 0, 0),
            publisher: "00".into(),
            abilities: vec![],
            capabilities: vec![],
        };
        let rec = bundle.to_app_record();
        assert_eq!(rec.id, "headless");
        assert_eq!(rec.launch_ability, "");
        assert!(rec.required_caps.is_empty());
    }

    #[cfg(nexus_env = "os")]
    #[test]
    fn backend_unavailable() {
        let service = Service::new();
        let err = service.install(InstallRequest { name: "launcher", manifest: b"" }).unwrap_err();
        assert_eq!(err, ServiceError::Unsupported);
    }
}
