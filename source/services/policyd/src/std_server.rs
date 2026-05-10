//! CONTEXT: Policy daemon domain library (service API and handlers)
//! INTENT: Policy/entitlement/DAC checks, audit
//! IDL (target): checkPermission(subject,cap), addPolicy(entry), audit(record)
//! DEPS: keystored/identityd (crypto/IDs)
//! READINESS: print "policyd: ready"; register/heartbeat with samgr
//! TESTS: checkPermission loopback; deny/allow paths
// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! policyd daemon: loads capability policies and answers allow/deny queries via Cap'n Proto IPC.

#![forbid(unsafe_code)]

use std::env;
use std::fmt;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use configd::{ConfigConsumer, ConsumerFailure, EffectiveSnapshot};
use nexus_ipc::{self, Wait};
use nexus_policy::{Decision, PolicyMode, PolicyTree, PolicyVersion};
use serde_json::{json, Value};
use thiserror::Error;

#[cfg(all(nexus_env = "host", nexus_env = "os"))]
compile_error!("nexus_env: both 'host' and 'os' set");

#[cfg(not(any(nexus_env = "host", nexus_env = "os")))]
compile_error!(
    "nexus_env: missing. Set RUSTFLAGS='--cfg nexus_env=\"host\"' or '--cfg nexus_env=\"os\"'.",
);

#[cfg(not(feature = "idl-capnp"))]
compile_error!("Enable the `idl-capnp` feature to build policyd handlers.");

#[cfg(feature = "idl-capnp")]
use capnp::message::{Builder, ReaderOptions};
#[cfg(feature = "idl-capnp")]
use capnp::serialize;
#[cfg(feature = "idl-capnp")]
use nexus_idl_runtime::policyd_capnp::{check_request, check_response};

const OPCODE_CHECK: u8 = 1;
const OPCODE_VERSION: u8 = 2;
const OPCODE_EVAL: u8 = 3;
const OPCODE_MODE_GET: u8 = 4;
const OPCODE_MODE_SET: u8 = 5;

/// Trait implemented by transports capable of delivering request frames to the daemon.
pub trait Transport {
    /// Error type surfaced by the transport implementation.
    type Error: Into<TransportError>;

    /// Receives the next request frame if one is available.
    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error>;

    /// Sends a response frame back to the caller.
    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error>;
}

/// Errors emitted by transports when interacting with the daemon.
#[derive(Debug)]
pub enum TransportError {
    /// Transport has been closed by the peer.
    Closed,
    /// I/O error while reading from or writing to the transport.
    Io(std::io::Error),
    /// Current platform lacks an implementation for the transport.
    Unsupported,
    /// Any other error described by a string message.
    Other(String),
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => write!(f, "transport closed"),
            Self::Io(err) => write!(f, "transport io error: {err}"),
            Self::Unsupported => write!(f, "transport unsupported"),
            Self::Other(msg) => write!(f, "transport error: {msg}"),
        }
    }
}

impl std::error::Error for TransportError {}

impl From<std::io::Error> for TransportError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<String> for TransportError {
    fn from(msg: String) -> Self {
        Self::Other(msg)
    }
}

impl From<&str> for TransportError {
    fn from(msg: &str) -> Self {
        Self::Other(msg.to_string())
    }
}

impl From<nexus_ipc::IpcError> for TransportError {
    fn from(err: nexus_ipc::IpcError) -> Self {
        match err {
            nexus_ipc::IpcError::Disconnected => Self::Closed,
            nexus_ipc::IpcError::Unsupported => Self::Unsupported,
            nexus_ipc::IpcError::WouldBlock | nexus_ipc::IpcError::Timeout => {
                Self::Other("operation timed out".to_string())
            }
            nexus_ipc::IpcError::NoSpace => Self::Other("ipc ran out of resources".to_string()),
            nexus_ipc::IpcError::Kernel(inner) => {
                Self::Other(format!("kernel ipc error: {inner:?}"))
            }
            _ => Self::Other(format!("ipc error: {err:?}")),
        }
    }
}

/// Notifies init that the daemon has completed its startup sequence.
pub struct ReadyNotifier(Box<dyn FnOnce() + Send>);

impl ReadyNotifier {
    /// Creates a notifier from the provided closure.
    pub fn new<F>(func: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self(Box::new(func))
    }

    /// Signals readiness to the caller.
    pub fn notify(self) {
        (self.0)();
    }
}

/// Transport backed by the [`nexus-ipc`] runtime.
pub struct IpcTransport<T> {
    server: T,
}

impl<T> IpcTransport<T> {
    /// Wraps a server implementation.
    pub fn new(server: T) -> Self {
        Self { server }
    }
}

impl<T> Transport for IpcTransport<T>
where
    T: nexus_ipc::Server + Send,
{
    type Error = nexus_ipc::IpcError;

    fn recv(&mut self) -> Result<Option<Vec<u8>>, Self::Error> {
        match self.server.recv(Wait::Blocking) {
            Ok(frame) => Ok(Some(frame)),
            Err(nexus_ipc::IpcError::Disconnected) => Ok(None),
            Err(nexus_ipc::IpcError::WouldBlock | nexus_ipc::IpcError::Timeout) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn send(&mut self, frame: &[u8]) -> Result<(), Self::Error> {
        self.server.send(frame, Wait::Blocking)
    }
}

/// Errors surfaced by the policy server.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Transport level failure.
    #[error("transport error: {0}")]
    Transport(TransportError),
    /// Failed to decode an incoming request frame.
    #[error("decode error: {0}")]
    Decode(String),
    /// Failed to encode an outgoing response frame.
    #[error("encode error: {0}")]
    Encode(#[from] capnp::Error),
    /// Failed to load policies from disk.
    #[error("policy error: {0}")]
    Policy(#[from] nexus_policy::Error),
    /// Failed to inspect the policy directory.
    #[error("init error: {0}")]
    Init(String),
}

#[derive(Debug, Error)]
pub enum PolicyLifecycleError {
    #[error("policy lifecycle transition is not authenticated")]
    Unauthorized,
    #[error("stale policy transition observed={observed} active={active}")]
    StaleVersion { observed: String, active: String },
    #[error("policy candidate rejected: {0}")]
    Candidate(#[from] nexus_policy::Error),
}

impl PolicyLifecycleError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unauthorized => "policy.lifecycle.unauthorized",
            Self::StaleVersion { .. } => "policy.lifecycle.stale",
            Self::Candidate(err) => err.code(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LifecycleAuth {
    pub actor_service_id: u64,
    pub can_manage_policy: bool,
    pub observed_version: String,
}

#[derive(Debug, Clone)]
pub struct PolicyReloadTxn {
    from_version: PolicyVersion,
    candidate: PolicyTree,
}

#[derive(Debug, Clone)]
pub struct VersionedDecision {
    pub version: String,
    pub decision: Decision,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyAuditEvent {
    pub action: &'static str,
    pub outcome: &'static str,
    pub version: String,
    pub actor_service_id: Option<u64>,
    pub subject: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct PolicyAuthority {
    active: PolicyTree,
    mode: PolicyMode,
    audit: Vec<PolicyAuditEvent>,
}

impl PolicyAuthority {
    pub fn new(active: PolicyTree) -> Self {
        Self {
            active,
            mode: PolicyMode::Enforce,
            audit: Vec::new(),
        }
    }

    pub fn active_version(&self) -> &str {
        self.active.version().as_str()
    }

    pub fn version(&mut self) -> String {
        let version = self.active_version().to_string();
        self.record_audit("version", "allow", None, None, "current");
        version
    }

    pub fn mode(&self) -> PolicyMode {
        self.mode
    }

    pub fn mode_get(&mut self) -> PolicyMode {
        let mode = self.mode;
        self.record_audit("mode_get", "allow", None, None, "current");
        mode
    }

    pub fn audit_log(&self) -> &[PolicyAuditEvent] {
        &self.audit
    }

    pub fn eval(
        &self,
        required: &[&str],
        subject: &str,
    ) -> Result<VersionedDecision, nexus_policy::Error> {
        let decision = self
            .active
            .policy()
            .evaluate(required, subject, self.mode)?;
        Ok(VersionedDecision {
            version: self.active_version().to_string(),
            decision,
        })
    }

    pub fn eval_audited(
        &mut self,
        required: &[&str],
        subject: &str,
    ) -> Result<VersionedDecision, nexus_policy::Error> {
        let result = self.eval(required, subject);
        match &result {
            Ok(versioned) => {
                let outcome = if versioned.decision.allow {
                    "allow"
                } else {
                    "deny"
                };
                self.record_audit(
                    "eval",
                    outcome,
                    None,
                    Some(subject.to_string()),
                    versioned.decision.reason_code.as_str(),
                );
            }
            Err(err) => {
                self.record_audit(
                    "eval",
                    "reject",
                    None,
                    Some(subject.to_string()),
                    err.code(),
                );
            }
        }
        result
    }

    pub fn prepare_reload_candidate(&mut self, candidate: PolicyTree) -> PolicyReloadTxn {
        self.record_audit("reload_prepare", "allow", None, None, "candidate_validated");
        PolicyReloadTxn {
            from_version: self.active.version().clone(),
            candidate,
        }
    }

    pub fn commit_reload(
        &mut self,
        txn: PolicyReloadTxn,
        auth: &LifecycleAuth,
    ) -> Result<(), PolicyLifecycleError> {
        if let Err(err) = self.ensure_authorized(auth) {
            self.record_audit(
                "reload_commit",
                "reject",
                Some(auth.actor_service_id),
                None,
                err.code(),
            );
            return Err(err);
        }
        if txn.from_version != *self.active.version() {
            let err = PolicyLifecycleError::StaleVersion {
                observed: txn.from_version.to_string(),
                active: self.active_version().to_string(),
            };
            self.record_audit(
                "reload_commit",
                "reject",
                Some(auth.actor_service_id),
                None,
                err.code(),
            );
            return Err(err);
        }
        self.active = txn.candidate;
        self.record_audit(
            "reload_commit",
            "allow",
            Some(auth.actor_service_id),
            None,
            "committed",
        );
        Ok(())
    }

    pub fn abort_reload(&mut self, _txn: PolicyReloadTxn) {
        self.record_audit("reload_abort", "reject", None, None, "configd_abort");
    }

    pub fn set_mode(
        &mut self,
        mode: PolicyMode,
        auth: &LifecycleAuth,
    ) -> Result<(), PolicyLifecycleError> {
        if let Err(err) = self.ensure_authorized(auth) {
            self.record_audit(
                "mode_set",
                "reject",
                Some(auth.actor_service_id),
                None,
                err.code(),
            );
            return Err(err);
        }
        self.mode = mode;
        self.record_audit(
            "mode_set",
            "allow",
            Some(auth.actor_service_id),
            None,
            "updated",
        );
        Ok(())
    }

    pub fn mode_set(
        &mut self,
        mode: PolicyMode,
        auth: &LifecycleAuth,
    ) -> Result<(), PolicyLifecycleError> {
        self.set_mode(mode, auth)
    }

    fn ensure_authorized(&self, auth: &LifecycleAuth) -> Result<(), PolicyLifecycleError> {
        if auth.actor_service_id == 0 || !auth.can_manage_policy {
            return Err(PolicyLifecycleError::Unauthorized);
        }
        if auth.observed_version != self.active_version() {
            return Err(PolicyLifecycleError::StaleVersion {
                observed: auth.observed_version.clone(),
                active: self.active_version().to_string(),
            });
        }
        Ok(())
    }

    fn record_audit(
        &mut self,
        action: &'static str,
        outcome: &'static str,
        actor_service_id: Option<u64>,
        subject: Option<String>,
        reason: &str,
    ) {
        self.audit.push(PolicyAuditEvent {
            action,
            outcome,
            version: self.active_version().to_string(),
            actor_service_id,
            subject,
            reason: reason.to_string(),
        });
    }
}

pub struct PolicyConfigConsumer {
    authority: Arc<Mutex<PolicyAuthority>>,
    actor_service_id: u64,
    prepared: Option<PolicyReloadTxn>,
}

impl PolicyConfigConsumer {
    pub fn new(authority: Arc<Mutex<PolicyAuthority>>, actor_service_id: u64) -> Self {
        Self {
            authority,
            actor_service_id,
            prepared: None,
        }
    }

    pub fn authority(&self) -> Arc<Mutex<PolicyAuthority>> {
        self.authority.clone()
    }
}

impl ConfigConsumer for PolicyConfigConsumer {
    fn prepare(&mut self, candidate: &EffectiveSnapshot) -> Result<(), ConsumerFailure> {
        let candidate = PolicyTree::load_root(Path::new(&candidate.effective.policy.root))
            .map_err(|err| ConsumerFailure::Reject(err.code().to_string()))?;
        let mut authority = self
            .authority
            .lock()
            .map_err(|_| ConsumerFailure::CommitFailed("policy_authority_lock".to_string()))?;
        self.prepared = Some(authority.prepare_reload_candidate(candidate));
        Ok(())
    }

    fn commit(&mut self, _candidate: &EffectiveSnapshot) -> Result<(), ConsumerFailure> {
        let Some(txn) = self.prepared.take() else {
            return Err(ConsumerFailure::CommitFailed(
                "policy_reload_without_prepare".to_string(),
            ));
        };
        let auth = LifecycleAuth {
            actor_service_id: self.actor_service_id,
            can_manage_policy: true,
            observed_version: txn.from_version.to_string(),
        };
        let mut authority = self
            .authority
            .lock()
            .map_err(|_| ConsumerFailure::CommitFailed("policy_authority_lock".to_string()))?;
        authority
            .commit_reload(txn, &auth)
            .map_err(|err| ConsumerFailure::CommitFailed(err.code().to_string()))
    }

    fn abort(&mut self, _candidate: &EffectiveSnapshot) {
        if let Some(txn) = self.prepared.take() {
            if let Ok(mut authority) = self.authority.lock() {
                authority.abort_reload(txn);
            }
        }
    }
}

impl From<TransportError> for ServerError {
    fn from(err: TransportError) -> Self {
        Self::Transport(err)
    }
}

struct PolicyService {
    authority: PolicyAuthority,
}

impl PolicyService {
    fn new(tree: PolicyTree) -> Self {
        Self {
            authority: PolicyAuthority::new(tree),
        }
    }

    fn handle_frame(&mut self, frame: &[u8]) -> Result<Vec<u8>, ServerError> {
        if frame.is_empty() {
            return Err(ServerError::Decode("empty request".to_string()));
        }
        match frame[0] {
            OPCODE_CHECK => self.handle_check(&frame[1..]),
            OPCODE_VERSION => self.handle_version(),
            OPCODE_EVAL => self.handle_eval_json(&frame[1..]),
            OPCODE_MODE_GET => self.handle_mode_get(),
            OPCODE_MODE_SET => self.handle_mode_set_json(&frame[1..]),
            other => Err(ServerError::Decode(format!("unknown opcode {other}"))),
        }
    }

    fn handle_version(&mut self) -> Result<Vec<u8>, ServerError> {
        encode_json_frame(
            OPCODE_VERSION,
            json!({ "version": self.authority.version() }),
        )
    }

    fn handle_mode_get(&mut self) -> Result<Vec<u8>, ServerError> {
        encode_json_frame(
            OPCODE_MODE_GET,
            json!({ "mode": mode_label(self.authority.mode_get()) }),
        )
    }

    fn handle_eval_json(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let value: Value = serde_json::from_slice(payload)
            .map_err(|err| ServerError::Decode(format!("eval json decode failed: {err}")))?;
        let subject = value
            .get("subject")
            .and_then(Value::as_str)
            .ok_or_else(|| ServerError::Decode("eval subject missing".to_string()))?;
        let caps = value
            .get("caps")
            .and_then(Value::as_array)
            .ok_or_else(|| ServerError::Decode("eval caps missing".to_string()))?
            .iter()
            .map(|cap| {
                cap.as_str()
                    .ok_or_else(|| ServerError::Decode("eval cap must be string".to_string()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let decision = self.authority.eval_audited(&caps, subject)?;
        encode_json_frame(
            OPCODE_EVAL,
            json!({ "version": decision.version, "decision": decision.decision }),
        )
    }

    fn handle_mode_set_json(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        let value: Value = serde_json::from_slice(payload)
            .map_err(|err| ServerError::Decode(format!("mode_set json decode failed: {err}")))?;
        let mode = parse_mode(
            value
                .get("set")
                .and_then(Value::as_str)
                .ok_or_else(|| ServerError::Decode("mode_set set missing".to_string()))?,
        )?;
        let actor_service_id = value
            .get("actor_service_id")
            .and_then(Value::as_u64)
            .ok_or_else(|| ServerError::Decode("mode_set actor_service_id missing".to_string()))?;
        let observed_version = value
            .get("observed_version")
            .and_then(Value::as_str)
            .ok_or_else(|| ServerError::Decode("mode_set observed_version missing".to_string()))?
            .to_string();
        let authorized = value
            .get("authorized")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let auth = LifecycleAuth {
            actor_service_id,
            can_manage_policy: authorized,
            observed_version,
        };
        self.authority.mode_set(mode, &auth).map_err(|err| {
            ServerError::Decode(format!("policy mode set rejected: {}", err.code()))
        })?;
        encode_json_frame(OPCODE_MODE_SET, json!({ "mode": mode_label(mode) }))
    }

    fn handle_check(&mut self, payload: &[u8]) -> Result<Vec<u8>, ServerError> {
        #[cfg(feature = "idl-capnp")]
        {
            let mut cursor = Cursor::new(payload);
            let message = serialize::read_message(&mut cursor, ReaderOptions::new())
                .map_err(|err| ServerError::Decode(format!("failed to read request: {err}")))?;
            let reader = message
                .get_root::<check_request::Reader<'_>>()
                .map_err(|err| {
                    ServerError::Decode(format!("failed to read request root: {err}"))
                })?;

            let subject = reader
                .get_subject()
                .map_err(|err| ServerError::Decode(format!("subject read error: {err}")))?;
            let subject = subject
                .to_str()
                .map_err(|err| ServerError::Decode(format!("subject utf8 error: {err}")))?;

            let caps_reader = reader
                .get_required_caps()
                .map_err(|err| ServerError::Decode(format!("required caps read error: {err}")))?;
            let mut caps = Vec::with_capacity(caps_reader.len() as usize);
            for idx in 0..caps_reader.len() {
                let cap = caps_reader
                    .get(idx)
                    .map_err(|err| ServerError::Decode(format!("cap read error: {err}")))?;
                let cap = cap
                    .to_str()
                    .map_err(|err| ServerError::Decode(format!("cap utf8 error: {err}")))?;
                caps.push(cap.to_string());
            }
            let cap_refs: Vec<&str> = caps.iter().map(String::as_str).collect();

            let mut message = Builder::new_default();
            {
                let mut response = message.init_root::<check_response::Builder<'_>>();
                match self.authority.eval_audited(&cap_refs, subject) {
                    Ok(versioned) if versioned.decision.allow => {
                        response.set_allowed(true);
                        response.reborrow().init_missing(0);
                    }
                    Ok(versioned) => {
                        response.set_allowed(false);
                        let missing_caps = versioned
                            .decision
                            .trace
                            .iter()
                            .filter(|step| !step.matched)
                            .map(|step| step.capability.as_str())
                            .collect::<Vec<_>>();
                        let mut missing =
                            response.reborrow().init_missing(missing_caps.len() as u32);
                        for (idx, cap) in missing_caps.iter().enumerate() {
                            missing.set(idx as u32, cap);
                        }
                    }
                    Err(err) => return Err(ServerError::Policy(err)),
                }
            }

            let mut body = Vec::new();
            serialize::write_message(&mut body, &message)?;
            let mut frame = Vec::with_capacity(1 + body.len());
            frame.push(OPCODE_CHECK);
            frame.extend_from_slice(&body);
            Ok(frame)
        }

        #[cfg(not(feature = "idl-capnp"))]
        {
            let _ = payload;
            Err(ServerError::Decode("capnp support disabled".to_string()))
        }
    }
}

fn encode_json_frame(opcode: u8, value: Value) -> Result<Vec<u8>, ServerError> {
    let body = serde_json::to_vec(&value)
        .map_err(|err| ServerError::Encode(capnp::Error::failed(err.to_string())))?;
    let mut frame = Vec::with_capacity(1 + body.len());
    frame.push(opcode);
    frame.extend_from_slice(&body);
    Ok(frame)
}

fn parse_mode(raw: &str) -> Result<PolicyMode, ServerError> {
    match raw {
        "enforce" => Ok(PolicyMode::Enforce),
        "dry-run" => Ok(PolicyMode::DryRun),
        "learn" => Ok(PolicyMode::Learn),
        other => Err(ServerError::Decode(format!("unknown policy mode {other}"))),
    }
}

fn mode_label(mode: PolicyMode) -> &'static str {
    match mode {
        PolicyMode::Enforce => "enforce",
        PolicyMode::DryRun => "dry-run",
        PolicyMode::Learn => "learn",
    }
}

/// Runs the daemon main loop using the default transport backend.
pub fn service_main_loop(notifier: ReadyNotifier) -> Result<(), ServerError> {
    #[cfg(nexus_env = "host")]
    {
        let (client, server) = nexus_ipc::loopback_channel();
        let _client_guard = client;
        let mut transport = IpcTransport::new(server);
        run_with_transport_ready(&mut transport, notifier)
    }

    #[cfg(nexus_env = "os")]
    {
        let server = nexus_ipc::KernelServer::new()
            .map_err(|err| ServerError::Transport(TransportError::from(err)))?;
        let mut transport = IpcTransport::new(server);
        run_with_transport_ready(&mut transport, notifier)
    }
}

/// Runs the daemon using the provided transport and emits readiness markers.
pub fn run_with_transport_ready<T: Transport>(
    transport: &mut T,
    notifier: ReadyNotifier,
) -> Result<(), ServerError> {
    let (mut service, files, subjects, caps) = load_policy_service()?;
    log_policy_counts(files, subjects, caps);
    notifier.notify();
    println!("policyd: ready");
    let _ = try_register_with_samgr();
    serve(&mut service, transport)
}

/// Runs the daemon using the provided transport without emitting readiness markers.
pub fn run_with_transport<T: Transport>(transport: &mut T) -> Result<(), ServerError> {
    let (mut service, files, subjects, caps) = load_policy_service()?;
    log_policy_counts(files, subjects, caps);
    serve(&mut service, transport)
}

fn serve<T>(service: &mut PolicyService, transport: &mut T) -> Result<(), ServerError>
where
    T: Transport,
{
    loop {
        match transport
            .recv()
            .map_err(|err| ServerError::Transport(err.into()))?
        {
            Some(frame) => {
                let response = service.handle_frame(&frame)?;
                transport
                    .send(&response)
                    .map_err(|err| ServerError::Transport(err.into()))?;
            }
            None => return Ok(()),
        }
    }
}

fn policy_dir() -> PathBuf {
    match env::var("NEXUS_POLICY_DIR") {
        Ok(path) => PathBuf::from(path),
        Err(_) => Path::new("policies").to_path_buf(),
    }
}

fn count_policy_files(dir: &Path) -> Result<usize, ServerError> {
    let entries = std::fs::read_dir(dir).map_err(|err| {
        ServerError::Init(format!(
            "failed to read policy dir {}: {err}",
            dir.display()
        ))
    })?;
    let mut count = 0;
    for entry in entries {
        let entry = entry.map_err(|err| {
            ServerError::Init(format!(
                "failed to read policy dir {}: {err}",
                dir.display()
            ))
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("toml") {
            count += 1;
        }
    }
    Ok(count)
}

fn load_policy_service() -> Result<(PolicyService, usize, usize, usize), ServerError> {
    let dir = policy_dir();
    let file_count = count_policy_files(&dir)?;
    let tree = PolicyTree::load_root(&dir)?;
    let subjects = tree.policy().subject_count();
    let caps = tree.policy().capability_count();
    Ok((PolicyService::new(tree), file_count, subjects, caps))
}

fn log_policy_counts(files: usize, subjects: usize, caps: usize) {
    println!("policyd: policies files={files} subjects={subjects} caps={caps}");
}

/// Attempts to register the daemon with `samgr` if a client is available.
fn try_register_with_samgr() -> Result<(), String> {
    // Placeholder: once shared IPC clients exist, register policyd with samgrd.
    Ok(())
}

/// Runs the daemon entry point until termination.
pub fn daemon_main<R: FnOnce() + Send + 'static>(notify: R) -> ! {
    touch_schemas();
    if let Err(err) = service_main_loop(ReadyNotifier::new(notify)) {
        eprintln!("policyd: {err}");
    }
    loop {
        core::hint::spin_loop();
    }
}

/// Creates a loopback transport pair for host-side tests.
#[cfg(nexus_env = "host")]
pub fn loopback_transport() -> (
    nexus_ipc::LoopbackClient,
    IpcTransport<nexus_ipc::LoopbackServer>,
) {
    let (client, server) = nexus_ipc::loopback_channel();
    (client, IpcTransport::new(server))
}

/// Touches the Cap'n Proto schema so release builds keep the generated module.
pub fn touch_schemas() {
    #[cfg(feature = "idl-capnp")]
    {
        let _ = core::any::type_name::<check_request::Reader<'static>>();
        let _ = core::any::type_name::<check_response::Reader<'static>>();
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::{
        LifecycleAuth, PolicyAuthority, PolicyConfigConsumer, PolicyService, OPCODE_CHECK,
        OPCODE_EVAL, OPCODE_MODE_GET, OPCODE_MODE_SET, OPCODE_VERSION,
    };
    use capnp::message::Builder;
    use capnp::serialize;
    use configd::Configd;
    use nexus_config::LayerInputs;
    use nexus_idl_runtime::policyd_capnp::{check_request, check_response};
    use nexus_policy::{PolicyMode, PolicyTree};
    use serde_json::json;
    use serde_json::Value;
    use std::fs;
    use std::io::Cursor;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    fn write_policy(root: &Path, caps: &[&str]) {
        fs::create_dir_all(root).expect("policy root");
        fs::write(
            root.join("nexus.policy.toml"),
            "version = 1\ninclude = ['base.toml']\n",
        )
        .expect("root");
        let caps = caps
            .iter()
            .map(|cap| format!("'{cap}'"))
            .collect::<Vec<_>>()
            .join(", ");
        fs::write(
            root.join("base.toml"),
            format!("[allow]\ndemo = [{caps}]\n"),
        )
        .expect("base");
    }

    fn auth(version: &str) -> LifecycleAuth {
        LifecycleAuth {
            actor_service_id: 42,
            can_manage_policy: true,
            observed_version: version.to_string(),
        }
    }

    fn base_layers() -> LayerInputs {
        LayerInputs {
            defaults: json!({
                "dsoftbus": { "transport": "auto", "max_peers": 32 },
                "metrics": { "enabled": true, "flush_interval_ms": 1000 },
                "tracing": { "level": "info", "sample_rate_per_mille": 100 },
                "security_sandbox": { "default_profile": "base", "max_caps": 16 },
                "sched": { "default_qos": "normal", "runqueue_slice_ms": 10 },
                "policy": { "root": "policies" }
            }),
            system: json!({}),
            state: json!({}),
            env: json!({}),
        }
    }

    fn decode_json_frame(frame: &[u8], expected_opcode: u8) -> Value {
        assert_eq!(frame.first().copied(), Some(expected_opcode));
        serde_json::from_slice(&frame[1..]).expect("json response")
    }

    fn encode_check_frame(subject: &str, caps: &[&str]) -> Vec<u8> {
        let mut message = Builder::new_default();
        {
            let mut request = message.init_root::<check_request::Builder<'_>>();
            request.set_subject(subject);
            let mut required = request.reborrow().init_required_caps(caps.len() as u32);
            for (idx, cap) in caps.iter().enumerate() {
                required.set(idx as u32, cap);
            }
        }
        let mut body = Vec::new();
        serialize::write_message(&mut body, &message).expect("encode request");
        let mut frame = Vec::with_capacity(1 + body.len());
        frame.push(OPCODE_CHECK);
        frame.extend_from_slice(&body);
        frame
    }

    fn decode_check_response(frame: &[u8]) -> (bool, Vec<String>) {
        assert_eq!(frame.first().copied(), Some(OPCODE_CHECK));
        let mut cursor = Cursor::new(&frame[1..]);
        let message = serialize::read_message(&mut cursor, Default::default()).expect("response");
        let reader = message
            .get_root::<check_response::Reader<'_>>()
            .expect("response root");
        let missing = reader.get_missing().expect("missing");
        let mut missing_caps = Vec::new();
        for idx in 0..missing.len() {
            missing_caps.push(
                missing
                    .get(idx)
                    .expect("missing cap")
                    .to_string()
                    .expect("utf8"),
            );
        }
        (reader.get_allowed(), missing_caps)
    }

    #[test]
    fn eval_returns_versioned_decision() {
        let temp = TempDir::new().expect("tempdir");
        write_policy(temp.path(), &["ipc.core"]);
        let tree = PolicyTree::load_root(temp.path()).expect("tree");
        let mut authority = PolicyAuthority::new(tree);

        let decision = authority.eval_audited(&["ipc.core"], "demo").expect("eval");

        assert_eq!(decision.version, authority.active_version());
        assert!(decision.decision.allow);
        assert_eq!(decision.decision.mode, PolicyMode::Enforce);
        assert_eq!(authority.audit_log().last().expect("audit").action, "eval");
    }

    #[test]
    fn reload_commit_replaces_active_version_after_authenticated_prepare() {
        let active_dir = TempDir::new().expect("active");
        let candidate_dir = TempDir::new().expect("candidate");
        write_policy(active_dir.path(), &["ipc.core"]);
        write_policy(candidate_dir.path(), &["ipc.core", "crypto.sign"]);
        let tree = PolicyTree::load_root(active_dir.path()).expect("tree");
        let candidate = PolicyTree::load_root(candidate_dir.path()).expect("candidate tree");
        let mut authority = PolicyAuthority::new(tree);
        let old_version = authority.active_version().to_string();

        let txn = authority.prepare_reload_candidate(candidate);
        authority
            .commit_reload(txn, &auth(&old_version))
            .expect("commit");

        assert_ne!(authority.active_version(), old_version);
        assert!(
            authority
                .eval(&["crypto.sign"], "demo")
                .expect("eval")
                .decision
                .allow
        );
    }

    #[test]
    fn configd_consumer_commits_policy_candidate_only_after_2pc_commit() {
        let active_dir = TempDir::new().expect("active");
        let candidate_dir = TempDir::new().expect("candidate");
        write_policy(active_dir.path(), &["ipc.core"]);
        write_policy(candidate_dir.path(), &["ipc.core", "crypto.sign"]);
        let tree = PolicyTree::load_root(active_dir.path()).expect("tree");
        let authority = Arc::new(Mutex::new(PolicyAuthority::new(tree)));
        let old_version = authority.lock().expect("lock").active_version().to_string();
        let mut configd = Configd::new(base_layers()).expect("configd");
        configd.register_consumer(Box::new(PolicyConfigConsumer::new(authority.clone(), 42)));

        let mut changed = base_layers();
        changed.state = json!({
            "policy": { "root": candidate_dir.path().display().to_string() }
        });
        let report = configd.reload(changed).expect("reload");

        let mut locked = authority.lock().expect("lock");
        assert!(report.committed);
        assert_ne!(locked.active_version(), old_version);
        assert!(
            locked
                .eval_audited(&["crypto.sign"], "demo")
                .expect("eval")
                .decision
                .allow
        );
        assert!(locked
            .audit_log()
            .iter()
            .any(|event| event.action == "reload_commit" && event.outcome == "allow"));
    }

    #[test]
    fn configd_consumer_reject_keeps_previous_policy_version_active() {
        let active_dir = TempDir::new().expect("active");
        let bad_dir = TempDir::new().expect("bad");
        write_policy(active_dir.path(), &["ipc.core"]);
        fs::create_dir_all(bad_dir.path()).expect("bad root");
        fs::write(bad_dir.path().join("nexus.policy.toml"), "version = 2\n").expect("bad");
        let tree = PolicyTree::load_root(active_dir.path()).expect("tree");
        let authority = Arc::new(Mutex::new(PolicyAuthority::new(tree)));
        let old_version = authority.lock().expect("lock").active_version().to_string();
        let mut configd = Configd::new(base_layers()).expect("configd");
        configd.register_consumer(Box::new(PolicyConfigConsumer::new(authority.clone(), 42)));

        let mut changed = base_layers();
        changed.state = json!({
            "policy": { "root": bad_dir.path().display().to_string() }
        });
        let report = configd.reload(changed).expect("reload");

        assert!(!report.committed);
        assert_eq!(
            report.reason.as_deref(),
            Some("prepare_reject:policy.invalid_root")
        );
        assert_eq!(
            authority.lock().expect("lock").active_version(),
            old_version
        );
    }

    #[test]
    fn reload_reject_keeps_previous_version_active() {
        let active_dir = TempDir::new().expect("active");
        let bad_dir = TempDir::new().expect("bad");
        write_policy(active_dir.path(), &["ipc.core"]);
        fs::create_dir_all(bad_dir.path()).expect("bad root");
        fs::write(bad_dir.path().join("nexus.policy.toml"), "version = 2\n").expect("bad");
        let tree = PolicyTree::load_root(active_dir.path()).expect("tree");
        let authority = PolicyAuthority::new(tree);
        let old_version = authority.active_version().to_string();

        let err = PolicyTree::load_root(bad_dir.path()).expect_err("reject");

        assert_eq!(err.code(), "policy.invalid_root");
        assert_eq!(authority.active_version(), old_version);
    }

    #[test]
    fn test_reject_unauthenticated_mode_change() {
        let temp = TempDir::new().expect("tempdir");
        write_policy(temp.path(), &["ipc.core"]);
        let tree = PolicyTree::load_root(temp.path()).expect("tree");
        let mut authority = PolicyAuthority::new(tree);
        let mut bad_auth = auth(authority.active_version());
        bad_auth.can_manage_policy = false;

        let err = authority
            .set_mode(PolicyMode::Learn, &bad_auth)
            .expect_err("reject");

        assert_eq!(err.code(), "policy.lifecycle.unauthorized");
        assert_eq!(authority.mode(), PolicyMode::Enforce);
        assert_eq!(
            authority.audit_log().last().expect("audit").outcome,
            "reject"
        );
    }

    #[test]
    fn test_reject_stale_mode_change() {
        let temp = TempDir::new().expect("tempdir");
        write_policy(temp.path(), &["ipc.core"]);
        let tree = PolicyTree::load_root(temp.path()).expect("tree");
        let mut authority = PolicyAuthority::new(tree);
        let stale_auth = auth("stale-version");

        let err = authority
            .set_mode(PolicyMode::DryRun, &stale_auth)
            .expect_err("reject");

        assert_eq!(err.code(), "policy.lifecycle.stale");
        assert_eq!(authority.mode(), PolicyMode::Enforce);
    }

    #[test]
    fn version_mode_and_set_api_are_audited() {
        let temp = TempDir::new().expect("tempdir");
        write_policy(temp.path(), &["ipc.core"]);
        let tree = PolicyTree::load_root(temp.path()).expect("tree");
        let mut authority = PolicyAuthority::new(tree);
        let version = authority.version();

        assert_eq!(authority.mode_get(), PolicyMode::Enforce);
        authority
            .mode_set(PolicyMode::DryRun, &auth(&version))
            .expect("mode set");

        assert_eq!(authority.mode(), PolicyMode::DryRun);
        assert!(authority
            .audit_log()
            .iter()
            .any(|event| event.action == "version" && event.outcome == "allow"));
        assert!(authority
            .audit_log()
            .iter()
            .any(|event| event.action == "mode_get" && event.outcome == "allow"));
        assert!(authority
            .audit_log()
            .iter()
            .any(|event| event.action == "mode_set" && event.outcome == "allow"));
    }

    #[test]
    fn external_frame_api_exposes_version_eval_mode_get_and_mode_set() {
        let temp = TempDir::new().expect("tempdir");
        write_policy(temp.path(), &["ipc.core"]);
        let tree = PolicyTree::load_root(temp.path()).expect("tree");
        let expected_version = tree.version().as_str().to_string();
        let mut service = PolicyService::new(tree);

        let version = service.handle_frame(&[OPCODE_VERSION]).expect("version");
        let version = decode_json_frame(&version, OPCODE_VERSION);
        assert_eq!(version["version"], expected_version);

        let eval_payload = serde_json::to_vec(&json!({
            "subject": "demo",
            "caps": ["ipc.core"]
        }))
        .expect("eval payload");
        let mut eval_frame = vec![OPCODE_EVAL];
        eval_frame.extend_from_slice(&eval_payload);
        let eval = service.handle_frame(&eval_frame).expect("eval");
        let eval = decode_json_frame(&eval, OPCODE_EVAL);
        assert_eq!(eval["decision"]["allow"], true);
        assert_eq!(eval["version"], expected_version);

        let mode = service.handle_frame(&[OPCODE_MODE_GET]).expect("mode");
        let mode = decode_json_frame(&mode, OPCODE_MODE_GET);
        assert_eq!(mode["mode"], "enforce");

        let set_payload = serde_json::to_vec(&json!({
            "set": "dry-run",
            "actor_service_id": 42,
            "observed_version": expected_version,
            "authorized": true
        }))
        .expect("set payload");
        let mut set_frame = vec![OPCODE_MODE_SET];
        set_frame.extend_from_slice(&set_payload);
        let set = service.handle_frame(&set_frame).expect("mode set");
        let set = decode_json_frame(&set, OPCODE_MODE_SET);
        assert_eq!(set["mode"], "dry-run");
    }

    #[test]
    fn service_check_frame_uses_unified_authority_for_allow_and_deny() {
        let temp = TempDir::new().expect("tempdir");
        write_policy(temp.path(), &["crypto.sign"]);
        let tree = PolicyTree::load_root(temp.path()).expect("tree");
        let mut service = PolicyService::new(tree);

        let allow = service
            .handle_frame(&encode_check_frame("demo", &["crypto.sign"]))
            .expect("allow");
        let deny = service
            .handle_frame(&encode_check_frame("demo", &["crypto.verify"]))
            .expect("deny");

        assert_eq!(decode_check_response(&allow), (true, Vec::new()));
        assert_eq!(
            decode_check_response(&deny),
            (false, vec!["crypto.verify".to_string()])
        );
        assert!(service
            .authority
            .audit_log()
            .iter()
            .any(|event| event.action == "eval" && event.outcome == "deny"));
    }
}
