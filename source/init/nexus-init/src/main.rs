// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Minimal init process responsible for launching core services and emitting
//! deterministic UART markers for the OS test harness.

#![forbid(unsafe_code)]
#![deny(clippy::all, missing_docs)]
#![allow(unexpected_cfgs)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use thiserror::Error;

const CORE_SERVICES: [&str; 4] = ["keystored", "policyd", "samgrd", "bundlemgrd"];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("init: fatal error: {err}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), InitError> {
    println!("init: start");
    let mut catalog = ServiceCatalog::load(Path::new("recipes/services"))?;
    catalog.ensure_core_defaults();

    let mut handles = Vec::new();
    for name in CORE_SERVICES {
        let config = catalog
            .get(name)
            .cloned()
            .ok_or_else(|| InitError::MissingService(name.to_string()))?;
        let handle = runtime::spawn_service(&config)?;
        handle.wait_ready()?;
        println!("{name}: up");
        handles.push(handle);
    }

    println!("init: ready");
    runtime::idle(handles)
}

#[derive(Clone, Debug, Deserialize)]
struct RawService {
    name: Option<String>,
    entry: Option<String>,
}

#[derive(Clone, Debug)]
struct ServiceConfig {
    name: String,
    entry: String,
}

impl ServiceConfig {
    fn new<N: Into<String>, E: Into<String>>(name: N, entry: E) -> Self {
        Self { name: name.into(), entry: entry.into() }
    }
}

struct ServiceCatalog {
    services: HashMap<String, ServiceConfig>,
}

impl ServiceCatalog {
    fn load(path: &Path) -> Result<Self, InitError> {
        let mut services = HashMap::new();
        if path.is_dir() {
            for entry in fs::read_dir(path)
                .map_err(|source| InitError::Io { path: path.to_path_buf(), source })?
            {
                let entry =
                    entry.map_err(|source| InitError::Io { path: path.to_path_buf(), source })?;
                let file_path = entry.path();
                if file_path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                    continue;
                }
                let raw = fs::read_to_string(&file_path)
                    .map_err(|source| InitError::Io { path: file_path.clone(), source })?;
                let recipe: RawService = toml::from_str(&raw)
                    .map_err(|source| InitError::Parse { path: file_path.clone(), source })?;
                let name = recipe.name.ok_or_else(|| InitError::InvalidRecipe {
                    path: file_path.clone(),
                    reason: "missing name".into(),
                })?;
                let entry = recipe.entry.unwrap_or_else(|| name.clone());
                let config = ServiceConfig::new(name.clone(), entry);
                if services.insert(name.clone(), config).is_some() {
                    return Err(InitError::DuplicateService(name));
                }
            }
        }
        Ok(Self { services })
    }

    fn ensure_core_defaults(&mut self) {
        for name in CORE_SERVICES {
            self.services.entry(name.to_string()).or_insert_with(|| ServiceConfig::new(name, name));
        }
    }

    fn get(&self, name: &str) -> Option<&ServiceConfig> {
        self.services.get(name)
    }
}

/// Error produced by the init runtime.
#[derive(Debug, Error)]
pub enum InitError {
    /// Failed to access a file or directory inside the recipe tree.
    #[error("failed to access {path}: {source}")]
    Io {
        /// Location associated with the error.
        path: PathBuf,
        /// Underlying operating system error.
        source: std::io::Error,
    },
    /// TOML parsing failed for a service recipe.
    #[error("failed to parse service recipe {path}: {source}")]
    Parse {
        /// Location of the malformed recipe file.
        path: PathBuf,
        /// Error returned by the TOML deserializer.
        source: toml::de::Error,
    },
    /// Recipe was missing mandatory metadata.
    #[error("invalid service recipe {path}: {reason}")]
    InvalidRecipe {
        /// Location of the malformed recipe file.
        path: PathBuf,
        /// Human readable description of the issue.
        reason: String,
    },
    /// Encountered the same service name multiple times while loading recipes.
    #[error("duplicate service definition for {0}")]
    DuplicateService(String),
    /// Spawning the service thread failed.
    #[error("service {name} spawn failed: {source}")]
    Spawn {
        /// Logical service name.
        name: String,
        /// Reason reported by the thread builder.
        source: std::io::Error,
    },
    /// Configuration referenced a service that could not be located.
    #[error("service {0} missing from catalog")]
    MissingService(String),
    /// Service failed to report readiness and terminated early.
    #[error("service {0} failed during startup")]
    ServiceFailed(String),
    /// Service reported a fatal runtime error.
    #[error("service {service} error: {detail}")]
    ServiceError {
        /// Name of the failing service.
        service: String,
        /// Human readable details from the daemon.
        detail: String,
    },
    /// Recipe referenced an entry point that is not supported yet.
    #[error("service {service} references unsupported entry {entry}")]
    UnsupportedEntry {
        /// Service that declared the entry.
        service: String,
        /// Requested entry symbol.
        entry: String,
    },
}

mod runtime {
    use super::{InitError, ServiceConfig};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;

    pub struct ServiceHandle {
        name: String,
        ready: Receiver<ServiceStatus>,
        #[allow(dead_code)]
        join: thread::JoinHandle<()>,
    }

    impl ServiceHandle {
        pub fn wait_ready(&self) -> Result<(), InitError> {
            match self.ready.recv() {
                Ok(ServiceStatus::Ready) => Ok(()),
                Ok(ServiceStatus::Failed(err)) => Err(err),
                Err(_) => Err(InitError::ServiceFailed(self.name.clone())),
            }
        }
    }

    pub enum ServiceStatus {
        Ready,
        Failed(InitError),
    }

    type ReadySender = Sender<ServiceStatus>;

    pub fn spawn_service(service: &ServiceConfig) -> Result<ServiceHandle, InitError> {
        let service = service.clone();
        let name = service.name.clone();
        let (tx, rx) = mpsc::channel();
        let join = thread::Builder::new()
            .name(format!("svc-{}", &name))
            .spawn(move || service_registry::launch(service, tx))
            .map_err(|source| InitError::Spawn { name: name.clone(), source })?;
        Ok(ServiceHandle { name, ready: rx, join })
    }

    pub fn idle(handles: Vec<ServiceHandle>) -> ! {
        let _handles = handles;
        loop {
            thread::park();
        }
    }

    mod service_registry {
        use super::{ReadySender, ServiceConfig, ServiceStatus};
        use crate::InitError;
        use keystored;
        use policyd;

        pub fn launch(service: ServiceConfig, ready: ReadySender) {
            let ServiceConfig { name, entry } = service;
            match entry.as_str() {
                "keystored" => {
                    let ready_clone = ready.clone();
                    let notifier = keystored::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready);
                    });
                    // Stub service: report readiness and idle.
                    let _ = keystored::service_main_loop(notifier);
                }
                "policyd" => {
                    let ready_clone = ready.clone();
                    let notifier = policyd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready);
                    });
                    // Stub service: report readiness and idle.
                    let _ = policyd::service_main_loop(notifier);
                }
                "samgrd" => {
                    let ready_clone = ready.clone();
                    let notifier = samgrd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready);
                    });
                    if let Err(err) = samgrd::service_main_loop(notifier) {
                        let detail = err.to_string();
                        let _ = ready.send(ServiceStatus::Failed(InitError::ServiceError {
                            service: name,
                            detail,
                        }));
                    }
                }
                "bundlemgrd" => {
                    let ready_clone = ready.clone();
                    let notifier = bundlemgrd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready);
                    });
                    let artifacts = bundlemgrd::ArtifactStore::new();
                    if let Err(err) = bundlemgrd::service_main_loop(notifier, artifacts) {
                        let detail = err.to_string();
                        let _ = ready.send(ServiceStatus::Failed(InitError::ServiceError {
                            service: name,
                            detail,
                        }));
                    }
                }
                other => {
                    let err =
                        InitError::UnsupportedEntry { service: name, entry: other.to_string() };
                    let _ = ready.send(ServiceStatus::Failed(err));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;

    #[test]
    fn loads_default_when_directory_missing() {
        let mut catalog = ServiceCatalog::load(Path::new("/non-existent/path")).unwrap();
        catalog.ensure_core_defaults();
        for name in CORE_SERVICES {
            assert!(catalog.get(name).is_some(), "missing core service {name}");
        }
    }

    #[test]
    fn parses_service_recipe() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file_path = dir.path().join("samgrd.toml");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "name = \"samgrd\"").unwrap();
        writeln!(file, "entry = \"samgrd-main\"").unwrap();
        drop(file);
        let mut catalog = ServiceCatalog::load(dir.path()).unwrap();
        catalog.ensure_core_defaults();
        let config = catalog.get("samgrd").unwrap();
        assert_eq!(config.entry, "samgrd-main");
    }
}
