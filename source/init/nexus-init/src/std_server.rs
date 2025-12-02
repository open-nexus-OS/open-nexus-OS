// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host std backend for service supervision and IPC
//! OWNERS: @init-team @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 unit tests
//!
//! PUBLIC API:
//!   - touch_schemas(): Warm schema caches
//!   - service_main_loop(): Main service supervision loop
//!   - ReadyNotifier: Readiness notification callback
//!
//! DEPENDENCIES:
//!   - std::thread: Thread management
//!   - nexus-ipc: IPC communication
//!   - Core services: keystored, policyd, samgrd, etc.
//!
//! ADR: docs/adr/0017-service-architecture.md
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use capnp::message::{Builder, HeapAllocator, ReaderOptions};
use capnp::serialize;
use nexus_idl_runtime::bundlemgr_capnp::{query_request, query_response};
use nexus_idl_runtime::execd_capnp::{exec_request, exec_response};
use nexus_idl_runtime::policyd_capnp::{check_request, check_response};
use nexus_ipc::{Client, Wait};
use std::io::Cursor;
use std::process::ExitCode;

const CORE_SERVICES: [&str; 7] = [
    "keystored",
    "policyd",
    "samgrd",
    "bundlemgrd",
    "packagefsd",
    "vfsd",
    "execd",
];

#[cfg(nexus_env = "host")]
const BUNDLE_OPCODE_QUERY: u8 = 2;
#[cfg(nexus_env = "host")]
const POLICY_OPCODE_CHECK: u8 = 1;
#[cfg(nexus_env = "host")]
const EXEC_OPCODE_EXEC: u8 = 1;

/// Touch schema registries for host builds so the runtime can serve
/// serialization requests without additional disk I/O during boot.
pub fn touch_schemas() {
    bundlemgrd::touch_schemas();
    execd::touch_schemas();
    packagefsd::touch_schemas();
    vfsd::touch_schemas();
    policyd::touch_schemas();
}

/// Callback invoked once all supervised services reach their ready markers.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Create a new notifier from the supplied closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Execute the wrapped callback.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Run the init supervisor and block until all core services are spawned.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), InitError> {
    run(Some(notifier))
}

fn run(notifier: Option<ReadyNotifier>) -> Result<(), InitError> {
    println!("init: start");
    let mut catalog = ServiceCatalog::load(Path::new("recipes/services"))?;
    catalog.ensure_core_defaults();

    let mut handles = Vec::new();
    let mut service_clients: HashMap<String, nexus_ipc::LoopbackClient> = HashMap::new();
    for name in CORE_SERVICES {
        let policy = core_restart_policy(name);
        println!("init: supervise {name} restart={policy}");
        let config = catalog
            .get(name)
            .cloned()
            .ok_or_else(|| InitError::MissingService(name.to_string()))?;
        let mut handle = runtime::spawn_service(&config)?;
        handle.wait_ready()?;
        if let Some(client) = handle.take_endpoint() {
            service_clients.insert(name.to_string(), client);
        }
        println!("init: up {name}");
        handles.push(handle);
    }

    let bundle_client = service_clients
        .remove("bundlemgrd")
        .map(BundleManagerClient::new);
    let policy_client = service_clients.remove("policyd").map(PolicyClient::new);
    let exec_client = service_clients.remove("execd").map(ExecClient::new);

    {
        let bundle_client = bundle_client
            .as_ref()
            .ok_or_else(|| service_error("bundlemgrd", "client unavailable"))?;
        let policy_client = policy_client
            .as_ref()
            .ok_or_else(|| service_error("policyd", "client unavailable"))?;
        let exec_client = exec_client
            .as_ref()
            .ok_or_else(|| service_error("execd", "client unavailable"))?;

        for name in catalog.non_core_names() {
            enforce_and_launch(&name, bundle_client, policy_client, exec_client)?;
        }
    }

    println!("init: ready");
    if let Some(notifier) = notifier {
        notifier.notify();
    }
    runtime::idle(handles)
}

fn core_restart_policy(name: &str) -> &'static str {
    match name {
        "keystored" | "policyd" | "samgrd" | "bundlemgrd" | "execd" => "always",
        _ => "never",
    }
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
        Self {
            name: name.into(),
            entry: entry.into(),
        }
    }
}

struct ServiceCatalog {
    services: HashMap<String, ServiceConfig>,
}

impl ServiceCatalog {
    fn load(path: &Path) -> Result<Self, InitError> {
        let mut services = HashMap::new();
        if path.is_dir() {
            for entry in fs::read_dir(path).map_err(|source| InitError::Io {
                path: path.to_path_buf(),
                source,
            })? {
                let entry = entry.map_err(|source| InitError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
                let file_path = entry.path();
                if file_path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
                    continue;
                }
                let raw = fs::read_to_string(&file_path).map_err(|source| InitError::Io {
                    path: file_path.clone(),
                    source,
                })?;
                let recipe: RawService =
                    toml::from_str(&raw).map_err(|source| InitError::Parse {
                        path: file_path.clone(),
                        source,
                    })?;
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
            self.services
                .entry(name.to_string())
                .or_insert_with(|| ServiceConfig::new(name, name));
        }
    }

    fn get(&self, name: &str) -> Option<&ServiceConfig> {
        self.services.get(name)
    }

    fn non_core_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .services
            .keys()
            .filter(|name| !CORE_SERVICES.contains(&name.as_str()))
            .cloned()
            .collect();
        names.sort();
        names
    }
}

struct BundleManagerClient {
    client: nexus_ipc::LoopbackClient,
}

struct BundleQuery {
    installed: bool,
    caps: Vec<String>,
}

impl BundleManagerClient {
    fn new(client: nexus_ipc::LoopbackClient) -> Self {
        Self { client }
    }

    fn query(&self, name: &str) -> Result<BundleQuery, InitError> {
        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<query_request::Builder<'_>>();
            request.set_name(name);
        }
        let frame = encode_frame(BUNDLE_OPCODE_QUERY, &message)
            .map_err(|err| service_error("bundlemgrd", format!("encode query: {err}")))?;
        self.client
            .send(&frame, Wait::Blocking)
            .map_err(|err| service_error("bundlemgrd", format!("send query: {err}")))?;
        let response = self
            .client
            .recv(Wait::Blocking)
            .map_err(|err| service_error("bundlemgrd", format!("recv query: {err}")))?;
        let (opcode, payload) = response
            .split_first()
            .ok_or_else(|| service_error("bundlemgrd", "empty query response"))?;
        if *opcode != BUNDLE_OPCODE_QUERY {
            return Err(service_error(
                "bundlemgrd",
                format!("unexpected opcode {opcode}"),
            ));
        }
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| service_error("bundlemgrd", format!("decode query: {err}")))?;
        let reader = message
            .get_root::<query_response::Reader<'_>>()
            .map_err(|err| service_error("bundlemgrd", format!("query root: {err}")))?;
        let caps_reader = reader
            .get_required_caps()
            .map_err(|err| service_error("bundlemgrd", format!("caps read: {err}")))?;
        let mut caps = Vec::with_capacity(caps_reader.len() as usize);
        for idx in 0..caps_reader.len() {
            let cap = caps_reader
                .get(idx)
                .map_err(|err| service_error("bundlemgrd", format!("cap[{idx}] read: {err}")))?;
            let text = cap
                .to_str()
                .map_err(|err| service_error("bundlemgrd", format!("cap[{idx}] utf8: {err}")))?;
            caps.push(text.to_string());
        }
        Ok(BundleQuery {
            installed: reader.get_installed(),
            caps,
        })
    }
}

struct PolicyClient {
    client: nexus_ipc::LoopbackClient,
}

enum PolicyOutcome {
    Allowed,
    Denied(Vec<String>),
}

impl PolicyClient {
    fn new(client: nexus_ipc::LoopbackClient) -> Self {
        Self { client }
    }

    fn check(&self, subject: &str, required: &[String]) -> Result<PolicyOutcome, InitError> {
        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<check_request::Builder<'_>>();
            request.set_subject(subject);
            let mut list = request.init_required_caps(required.len() as u32);
            for (idx, cap) in required.iter().enumerate() {
                list.set(idx as u32, cap);
            }
        }
        let frame = encode_frame(POLICY_OPCODE_CHECK, &message)
            .map_err(|err| service_error("policyd", format!("encode check: {err}")))?;
        self.client
            .send(&frame, Wait::Blocking)
            .map_err(|err| service_error("policyd", format!("send check: {err}")))?;
        let response = self
            .client
            .recv(Wait::Blocking)
            .map_err(|err| service_error("policyd", format!("recv check: {err}")))?;
        let (opcode, payload) = response
            .split_first()
            .ok_or_else(|| service_error("policyd", "empty check response"))?;
        if *opcode != POLICY_OPCODE_CHECK {
            return Err(service_error(
                "policyd",
                format!("unexpected opcode {opcode}"),
            ));
        }
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| service_error("policyd", format!("decode check: {err}")))?;
        let reader = message
            .get_root::<check_response::Reader<'_>>()
            .map_err(|err| service_error("policyd", format!("check root: {err}")))?;
        if reader.get_allowed() {
            Ok(PolicyOutcome::Allowed)
        } else {
            let missing_reader = reader
                .get_missing()
                .map_err(|err| service_error("policyd", format!("missing read: {err}")))?;
            let mut missing = Vec::with_capacity(missing_reader.len() as usize);
            for idx in 0..missing_reader.len() {
                let cap = missing_reader.get(idx).map_err(|err| {
                    service_error("policyd", format!("missing[{idx}] read: {err}"))
                })?;
                let text = cap.to_str().map_err(|err| {
                    service_error("policyd", format!("missing[{idx}] utf8: {err}"))
                })?;
                missing.push(text.to_string());
            }
            Ok(PolicyOutcome::Denied(missing))
        }
    }
}

struct ExecClient {
    client: nexus_ipc::LoopbackClient,
}

impl ExecClient {
    fn new(client: nexus_ipc::LoopbackClient) -> Self {
        Self { client }
    }

    fn exec(&self, name: &str) -> Result<(), InitError> {
        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<exec_request::Builder<'_>>();
            request.set_name(name);
        }
        let frame = encode_frame(EXEC_OPCODE_EXEC, &message)
            .map_err(|err| service_error("execd", format!("encode exec: {err}")))?;
        self.client
            .send(&frame, Wait::Blocking)
            .map_err(|err| service_error("execd", format!("send exec: {err}")))?;
        let response = self
            .client
            .recv(Wait::Blocking)
            .map_err(|err| service_error("execd", format!("recv exec: {err}")))?;
        let (opcode, payload) = response
            .split_first()
            .ok_or_else(|| service_error("execd", "empty exec response"))?;
        if *opcode != EXEC_OPCODE_EXEC {
            return Err(service_error(
                "execd",
                format!("unexpected opcode {opcode}"),
            ));
        }
        let mut cursor = Cursor::new(payload);
        let message = serialize::read_message(&mut cursor, ReaderOptions::new())
            .map_err(|err| service_error("execd", format!("decode exec: {err}")))?;
        let reader = message
            .get_root::<exec_response::Reader<'_>>()
            .map_err(|err| service_error("execd", format!("exec root: {err}")))?;
        if reader.get_ok() {
            Ok(())
        } else {
            let detail = reader
                .get_message()
                .ok()
                .and_then(|m| m.to_str().ok())
                .unwrap_or("".into());
            Err(service_error("execd", detail))
        }
    }
}

fn enforce_and_launch(
    name: &str,
    bundle: &BundleManagerClient,
    policy: &PolicyClient,
    execd: &ExecClient,
) -> Result<(), InitError> {
    let query = bundle.query(name)?;
    if !query.installed {
        println!("init: deny {name} (not installed)");
        return Ok(());
    }
    match policy.check(name, &query.caps)? {
        PolicyOutcome::Allowed => {
            println!("init: allow {name}");
            execd.exec(name)?;
        }
        PolicyOutcome::Denied(missing) => {
            if missing.is_empty() {
                println!("init: deny {name} (denied)");
            } else {
                println!("init: deny {name} missing={}", missing.join(","));
            }
        }
    }
    Ok(())
}

fn encode_frame(opcode: u8, message: &Builder<HeapAllocator>) -> Result<Vec<u8>, capnp::Error> {
    let mut payload = Vec::new();
    serialize::write_message(&mut payload, message)?;
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(opcode);
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn service_error(service: &str, detail: impl Into<String>) -> InitError {
    InitError::ServiceError {
        service: service.to_string(),
        detail: detail.into(),
    }
}

/// Error produced by the init runtime while supervising services.
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

    type ServiceClient = nexus_ipc::LoopbackClient;

    pub struct ServiceHandle {
        name: String,
        ready: Receiver<ServiceStatus>,
        #[allow(dead_code)]
        join: thread::JoinHandle<()>,
        endpoint: Option<ServiceClient>,
    }

    impl ServiceHandle {
        pub fn wait_ready(&mut self) -> Result<(), InitError> {
            match self.ready.recv() {
                Ok(ServiceStatus::Ready(endpoint)) => {
                    self.endpoint = endpoint;
                    Ok(())
                }
                Ok(ServiceStatus::Failed(err)) => Err(err),
                Err(_) => Err(InitError::ServiceFailed(self.name.clone())),
            }
        }

        pub fn take_endpoint(&mut self) -> Option<ServiceClient> {
            self.endpoint.take()
        }
    }

    pub enum ServiceStatus {
        Ready(Option<ServiceClient>),
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
            .map_err(|source| InitError::Spawn {
                name: name.clone(),
                source,
            })?;
        Ok(ServiceHandle {
            name,
            ready: rx,
            join,
            endpoint: None,
        })
    }

    pub fn idle(handles: Vec<ServiceHandle>) -> ! {
        let _handles = handles;
        loop {
            thread::park();
        }
    }

    mod service_registry {
        use super::{ReadySender, ServiceConfig, ServiceStatus};
        use crate::std_server::InitError;

        pub fn launch(service: ServiceConfig, ready: ReadySender) {
            let ServiceConfig { name, entry } = service;
            match entry.as_str() {
                "keystored" => {
                    let ready_clone = ready.clone();
                    let notifier = keystored::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(None));
                    });
                    let _ = keystored::service_main_loop(notifier);
                }
                "policyd" => {
                    policyd::touch_schemas();
                    let ready_clone = ready.clone();
                    let service_name = name.clone();
                    let (client, server) = nexus_ipc::loopback_channel();
                    let mut transport = policyd::IpcTransport::new(server);
                    let notifier = policyd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(Some(client)));
                    });
                    if let Err(err) = policyd::run_with_transport_ready(&mut transport, notifier) {
                        let detail = err.to_string();
                        let _ = ready.send(ServiceStatus::Failed(InitError::ServiceError {
                            service: service_name,
                            detail,
                        }));
                    }
                }
                "samgrd" => {
                    let ready_clone = ready.clone();
                    let notifier = samgrd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(None));
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
                    bundlemgrd::touch_schemas();
                    let ready_clone = ready.clone();
                    let service_name = name.clone();
                    let artifacts = bundlemgrd::ArtifactStore::new();
                    let (bundle_client, bundle_server) = nexus_ipc::loopback_channel();
                    let (keystore_client, keystore_server) = nexus_ipc::loopback_channel();
                    std::thread::spawn(move || {
                        let mut ks_transport = keystored::IpcTransport::new(keystore_server);
                        let _ = keystored::run_with_transport_default_anchors(&mut ks_transport);
                    });
                    let mut transport = bundlemgrd::IpcTransport::new(bundle_server);
                    let keystore = Some(bundlemgrd::KeystoreHandle::from_loopback(keystore_client));
                    let notifier = bundlemgrd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(Some(bundle_client)));
                    });
                    notifier.notify();
                    if let Err(err) =
                        bundlemgrd::run_with_transport(&mut transport, artifacts, keystore, None)
                    {
                        let detail = err.to_string();
                        let _ = ready.send(ServiceStatus::Failed(InitError::ServiceError {
                            service: service_name,
                            detail,
                        }));
                    }
                }
                "packagefsd" => {
                    #[cfg(nexus_env = "os")]
                    nexus_ipc::set_default_target("packagefsd");
                    let ready_clone = ready.clone();
                    let notifier = packagefsd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(None));
                    });
                    let _ = packagefsd::service_main_loop(notifier);
                }
                "vfsd" => {
                    #[cfg(nexus_env = "os")]
                    nexus_ipc::set_default_target("vfsd");
                    let ready_clone = ready.clone();
                    let notifier = vfsd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(None));
                    });
                    let _ = vfsd::service_main_loop(notifier);
                }
                "execd" => {
                    execd::touch_schemas();
                    let ready_clone = ready.clone();
                    let service_name = name.clone();
                    let (client, server) = nexus_ipc::loopback_channel();
                    let mut transport = execd::IpcTransport::new(server);
                    let notifier = execd::ReadyNotifier::new(move || {
                        let _ = ready_clone.send(ServiceStatus::Ready(Some(client)));
                    });
                    if let Err(err) = execd::run_with_transport_ready(&mut transport, notifier) {
                        let detail = err.to_string();
                        let _ = ready.send(ServiceStatus::Failed(InitError::ServiceError {
                            service: service_name,
                            detail,
                        }));
                    }
                }
                other => {
                    let err = InitError::UnsupportedEntry {
                        service: name,
                        entry: other.to_string(),
                    };
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
