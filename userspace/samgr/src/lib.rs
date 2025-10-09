// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Userspace service manager facade used by host-first tests.
//!
//! When compiled with `nexus_env="host"` (via RUSTFLAGS), this crate provides
//! an in-memory registry suitable for exercising restart and heartbeat semantics
//! without the kernel. The `nexus_env="os"` configuration is a placeholder for
//! future syscall wiring and currently returns [`Error::Unsupported`] for all operations.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!("nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '...\"os\"'");

pub mod cli;
pub use cli::{execute, help, run};

use std::fmt;
/// Minimal remote routing hook implemented by DSoftBus-lite.
pub trait RemoteRouter {
    /// Returns true if the remote node at `device_id` can route `service`.
    fn resolve_remote(&self, device_id: &str, service: &str) -> bool;
}

#[cfg(nexus_env = "host")]
use parking_lot::Mutex;
#[cfg(nexus_env = "host")]
use std::{collections::HashMap, time::Instant};

/// Result alias for service manager operations.
pub type Result<T> = core::result::Result<T, Error>;

/// Errors produced by the service manager.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    /// A service with the provided name already exists.
    #[error("service already registered")]
    Duplicate,
    /// The requested service does not exist.
    #[error("service not found")]
    NotFound,
    /// A handle refers to an outdated generation after a restart.
    #[error("stale service handle")]
    StaleHandle,
    /// Backend is not implemented for this build configuration.
    #[error("operation unsupported for this backend")]
    Unsupported,
}

/// Unique generation identifier assigned to each service registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u64);

impl Generation {
    const fn first() -> Self {
        Self(1)
    }

    /// Returns the next monotonically increasing generation value.
    fn next(self) -> Self {
        Self(self.0 + 1)
    }

    /// Exposes the raw numeric value primarily for testing.
    pub fn value(self) -> u64 {
        self.0
    }
}

/// Endpoint identifier describing how to reach a service instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Endpoint {
    address: String,
}

impl Endpoint {
    /// Creates a new endpoint wrapper from the provided address string.
    pub fn new(address: impl Into<String>) -> Self {
        Self { address: address.into() }
    }

    /// Returns the raw endpoint address.
    pub fn as_str(&self) -> &str {
        &self.address
    }
}

impl From<&str> for Endpoint {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl fmt::Display for Endpoint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.address)
    }
}

/// Handle returned by registration and resolution requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceHandle {
    /// Human readable name of the service.
    pub name: String,
    /// Endpoint to reach the current instance.
    pub endpoint: Endpoint,
    /// Monotonic generation associated with the instance.
    pub generation: Generation,
}

impl ServiceHandle {
    fn new(name: String, endpoint: Endpoint, generation: Generation) -> Self {
        Self { name, endpoint, generation }
    }
}

/// Primary entry point for interacting with the service manager backend.
#[derive(Default)]
pub struct Registry {
    #[cfg(nexus_env = "host")]
    host: HostRegistry,
    /// Optional remote resolver that can be used when a device id is specified.
    #[cfg(nexus_env = "host")]
    remote: Option<Box<dyn RemoteRouter + Send + Sync>>,
}

// (Default for Registry is derived when host backend is enabled)

// OS backend keeps Unsupported stubs; default is derived only for host builds

impl Registry {
    /// Creates a new registry using the selected backend.
    pub fn new() -> Self {
        Self::default()
    }

    /// Installs a remote router used to resolve services by device id.
    #[cfg(nexus_env = "host")]
    pub fn with_remote_router(mut self, router: Box<dyn RemoteRouter + Send + Sync>) -> Self {
        self.remote = Some(router);
        self
    }
}

#[cfg(nexus_env = "host")]
impl Registry {
    /// Registers a service if it is currently unknown.
    pub fn register(&self, name: impl Into<String>, endpoint: Endpoint) -> Result<ServiceHandle> {
        self.host.register(name.into(), endpoint)
    }

    /// Resolves the latest endpoint for the service `name`.
    pub fn resolve(&self, name: &str) -> Result<ServiceHandle> {
        // Support a minimal routing convention: "device_id:service". If a
        // remote router is present, attempt a remote resolution first.
        if let Some((device, service)) = name.split_once(":") {
            if let Some(router) = &self.remote {
                if router.resolve_remote(device, service) {
                    // Indicate a routed endpoint through a synthetic address.
                    let endpoint = Endpoint::new(format!("dsoftbus://{device}/{service}"));
                    return Ok(ServiceHandle::new(
                        service.to_string(),
                        endpoint,
                        Generation::first(),
                    ));
                }
            }
            return Err(Error::NotFound);
        }
        self.host.resolve(name)
    }

    /// Records a heartbeat for the provided service handle.
    pub fn heartbeat(&self, handle: &ServiceHandle) -> Result<()> {
        self.host.heartbeat(handle)
    }

    /// Marks the service as restarted and swaps in `endpoint` as the new location.
    pub fn restart(&self, name: &str, endpoint: Endpoint) -> Result<ServiceHandle> {
        self.host.restart(name, endpoint)
    }
}

#[cfg(nexus_env = "os")]
impl Registry {
    /// Registers a service; currently unsupported.
    pub fn register(&self, _name: impl Into<String>, _endpoint: Endpoint) -> Result<ServiceHandle> {
        Err(Error::Unsupported)
    }

    /// Resolves a service; currently unsupported.
    pub fn resolve(&self, _name: &str) -> Result<ServiceHandle> {
        Err(Error::Unsupported)
    }

    /// Heartbeats are unsupported on the OS backend stub.
    pub fn heartbeat(&self, _handle: &ServiceHandle) -> Result<()> {
        Err(Error::Unsupported)
    }

    /// Restarts are unsupported on the OS backend stub.
    pub fn restart(&self, _name: &str, _endpoint: Endpoint) -> Result<ServiceHandle> {
        Err(Error::Unsupported)
    }
}

#[cfg(nexus_env = "host")]
#[derive(Default)]
struct HostRegistry {
    services: Mutex<HashMap<String, ServiceRecord>>,
}

#[cfg(nexus_env = "host")]
struct ServiceRecord {
    endpoint: Endpoint,
    generation: Generation,
    last_heartbeat: Instant,
}

#[cfg(nexus_env = "host")]
impl HostRegistry {
    fn register(&self, name: String, endpoint: Endpoint) -> Result<ServiceHandle> {
        let mut services = self.services.lock();
        if services.contains_key(&name) {
            return Err(Error::Duplicate);
        }
        let generation = Generation::first();
        let record = ServiceRecord {
            endpoint: endpoint.clone(),
            generation,
            last_heartbeat: Instant::now(),
        };
        services.insert(name.clone(), record);
        Ok(ServiceHandle::new(name, endpoint, generation))
    }

    fn resolve(&self, name: &str) -> Result<ServiceHandle> {
        let services = self.services.lock();
        let record = services.get(name).ok_or(Error::NotFound)?;
        Ok(ServiceHandle::new(name.to_string(), record.endpoint.clone(), record.generation))
    }

    fn heartbeat(&self, handle: &ServiceHandle) -> Result<()> {
        let mut services = self.services.lock();
        let record = services.get_mut(&handle.name).ok_or(Error::NotFound)?;
        if record.generation != handle.generation {
            return Err(Error::StaleHandle);
        }
        record.last_heartbeat = Instant::now();
        Ok(())
    }

    fn restart(&self, name: &str, endpoint: Endpoint) -> Result<ServiceHandle> {
        let mut services = self.services.lock();
        let record = services.get_mut(name).ok_or(Error::NotFound)?;
        record.generation = record.generation.next();
        record.endpoint = endpoint.clone();
        record.last_heartbeat = Instant::now();
        Ok(ServiceHandle::new(name.to_string(), endpoint, record.generation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[cfg(nexus_env = "host")]
    #[test]
    fn register_and_resolve_roundtrip() {
        let registry = Registry::new();
        let handle =
            registry.register("samgr", Endpoint::from("ipc://samgr")).expect("register succeeds");
        let resolved = registry.resolve("samgr").expect("resolve succeeds");
        assert_eq!(handle, resolved);
        registry.heartbeat(&resolved).expect("heartbeat ok");
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn duplicate_registration_rejected() {
        let registry = Registry::new();
        registry.register("samgr", Endpoint::from("ipc://samgr")).expect("initial register");
        let err = registry
            .register("samgr", Endpoint::from("ipc://samgr2"))
            .expect_err("duplicate rejected");
        assert_eq!(err, Error::Duplicate);
    }

    #[cfg(nexus_env = "host")]
    #[test]
    fn restart_invalidates_old_handle() {
        let registry = Registry::new();
        let handle = registry.register("samgr", Endpoint::from("ipc://samgr")).expect("register");
        let restarted =
            registry.restart("samgr", Endpoint::from("ipc://samgr-new")).expect("restart");
        assert!(restarted.generation.value() > handle.generation.value());
        assert_eq!(registry.resolve("samgr").unwrap(), restarted);
        let err = registry.heartbeat(&handle).expect_err("old handle rejected");
        assert_eq!(err, Error::StaleHandle);
    }

    // Under Miri, proptest is very slow and uses OS APIs (cwd). Provide a
    // lightweight deterministic variant and keep the property test for normal runs.

    #[cfg(all(nexus_env = "host", not(miri)))]
    proptest! {
        #[test]
        fn restart_sequence_updates_generation(endpoints in proptest::collection::vec("[a-z0-9]{3,8}", 1..6)) {
            let registry = Registry::new();
            let mut iter = endpoints.into_iter();
            let first = iter.next().unwrap_or_else(|| "svc0".to_string());
            let mut handle = registry.register("svc", Endpoint::new(first.clone())).unwrap();
            let mut last_endpoint = handle.endpoint.clone();
            for ep in iter {
                let next = registry.restart("svc", Endpoint::new(ep.clone())).unwrap();
                prop_assert!(next.generation.value() > handle.generation.value());
                handle = next.clone();
                last_endpoint = Endpoint::new(ep);
            }
            let resolved = registry.resolve("svc").unwrap();
            prop_assert_eq!(resolved.endpoint, last_endpoint);
            prop_assert_eq!(resolved.generation, handle.generation);
        }
    }

    #[cfg(all(nexus_env = "host", miri))]
    #[test]
    fn restart_sequence_updates_generation_miri_smoke() {
        let registry = Registry::new();
        let mut handle = registry.register("svc", Endpoint::new("a1")).unwrap();
        for ep in ["b2", "c3", "d4"] {
            let next = registry.restart("svc", Endpoint::new(ep)).unwrap();
            assert!(next.generation.value() > handle.generation.value());
            handle = next;
        }
        let resolved = registry.resolve("svc").unwrap();
        assert_eq!(resolved.generation.value(), handle.generation.value());
    }
}
