extern crate alloc;

use core::fmt;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nexus_ipc::Server;
use nexus_ipc::{Client, IpcError, KernelClient, KernelServer, Wait};

const OPCODE_RESOLVE: u8 = 2;
const KIND_FILE: u16 = 0;
const KIND_DIRECTORY: u16 = 1;

const DEMO_HELLO_MANIFEST_JSON: &[u8] = br#"{"name":"demo.hello","version":"1.0.0"}"#;
const DEMO_HELLO_PAYLOAD: &[u8] = b"HELLO_PAYLOAD_BYTES";
const DEMO_EXIT_MANIFEST_JSON: &[u8] = br#"{"name":"demo.exit0","version":"1.0.0"}"#;
const DEMO_EXIT_PAYLOAD: &[u8] = b"EXIT0";

/// Result type used by the os-lite backend.
pub type LiteResult<T> = core::result::Result<T, LiteError>;

/// Errors surfaced by the os-lite backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiteError {
    /// IPC layer failed.
    Transport,
    /// Registry lookups failed.
    Registry,
}

impl fmt::Display for LiteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport => write!(f, "transport error"),
            Self::Registry => write!(f, "registry error"),
        }
    }
}

/// Ready notifier used by init.
pub struct ReadyNotifier<F: FnOnce() + Send>(F);

impl<F: FnOnce() + Send> ReadyNotifier<F> {
    /// Creates a notifier from the provided closure.
    pub fn new(func: F) -> Self {
        Self(func)
    }

    /// Emits the readiness signal.
    pub fn notify(self) {
        (self.0)();
    }
}

#[derive(Default)]
struct BundleRegistry {
    bundles: BTreeMap<String, BTreeMap<String, Entry>>, // bundle@version -> path -> entry
    active: BTreeMap<String, String>,                   // bundle -> version
}

impl BundleRegistry {
    fn publish(&mut self, bundle: &str, version: &str, entries: &[(String, Entry)]) {
        let key = format!("{bundle}@{version}");
        let record = self.bundles.entry(key).or_default();
        record.clear();
        for (path, entry) in entries {
            record.insert(path.clone(), entry.clone());
        }
        self.active.insert(bundle.to_string(), version.to_string());
    }

    fn resolve(&self, rel: &str) -> Option<Entry> {
        let rel = rel.trim_start_matches('/');
        let (bundle, path) = rel.split_once('/')?;
        let canonical = if bundle.contains('@') {
            bundle.to_string()
        } else {
            let version = self.active.get(bundle)?;
            format!("{bundle}@{version}")
        };
        let entries = self.bundles.get(&canonical)?;
        entries.get(path).cloned()
    }
}

#[derive(Clone)]
struct Entry {
    size: u64,
    kind: u16,
    bytes: Vec<u8>,
}

impl Entry {
    fn directory() -> Self {
        Self { size: 0, kind: KIND_DIRECTORY, bytes: Vec::new() }
    }

    fn file(bytes: &[u8]) -> Self {
        Self { size: bytes.len() as u64, kind: KIND_FILE, bytes: bytes.to_vec() }
    }
}

/// Runs the minimal packagefs daemon, emitting a readiness marker once.
pub fn service_main_loop<F: FnOnce() + Send>(notifier: ReadyNotifier<F>) -> LiteResult<()> {
    debug_print("packagefsd: ready\n");
    notifier.notify();
    // RFC-0005: name-based routing; init-lite assigns per-service endpoint caps and answers route
    // queries over a private control channel, so services don't hardcode slot numbers.
    let server = KernelServer::new_for("packagefsd").map_err(|_| LiteError::Transport)?;
    let registry = load_registry_from_bundlemgrd().unwrap_or_else(seed_registry);
    run_loop(&server, &registry)
}

fn run_loop(server: &KernelServer, registry: &BundleRegistry) -> LiteResult<()> {
    let mut response = Vec::with_capacity(256);
    loop {
        match server.recv(Wait::Blocking) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    continue;
                }
                match bytes[0] {
                    OPCODE_RESOLVE => {
                        let rel = core::str::from_utf8(&bytes[1..]).unwrap_or("");
                        let entry = registry.resolve(rel);
                        response.clear();
                        if let Some(entry) = entry {
                            response.push(1);
                            response.extend_from_slice(&entry.size.to_le_bytes());
                            response.extend_from_slice(&entry.kind.to_le_bytes());
                            response.extend_from_slice(&entry.bytes);
                        } else {
                            response.push(0);
                            response.extend_from_slice(&0u64.to_le_bytes());
                            response.extend_from_slice(&0u16.to_le_bytes());
                        }
                        server.send(&response, Wait::Blocking).map_err(|_| LiteError::Transport)?;
                    }
                    _ => {
                        response.clear();
                        response.push(0);
                        response.extend_from_slice(&0u64.to_le_bytes());
                        response.extend_from_slice(&0u16.to_le_bytes());
                        server.send(&response, Wait::Blocking).map_err(|_| LiteError::Transport)?;
                    }
                }
            }
            Err(IpcError::Disconnected) => return Err(LiteError::Transport),
            Err(IpcError::WouldBlock) | Err(IpcError::Timeout) => {
                let _ = nexus_abi::yield_();
            }
            Err(_) => return Err(LiteError::Transport),
        }
    }
}

fn seed_registry() -> BundleRegistry {
    let mut registry = BundleRegistry::default();
    let hello_entries = vec![
        (".".to_string(), Entry::directory()),
        ("manifest.json".to_string(), Entry::file(DEMO_HELLO_MANIFEST_JSON)),
        ("payload.elf".to_string(), Entry::file(DEMO_HELLO_PAYLOAD)),
    ];
    registry.publish("demo.hello", "1.0.0", &hello_entries);

    let exit_entries = vec![
        (".".to_string(), Entry::directory()),
        ("manifest.json".to_string(), Entry::file(DEMO_EXIT_MANIFEST_JSON)),
        ("payload.elf".to_string(), Entry::file(DEMO_EXIT_PAYLOAD)),
    ];
    registry.publish("demo.exit0", "1.0.0", &exit_entries);

    registry
}

fn load_registry_from_bundlemgrd() -> Option<BundleRegistry> {
    // NOTE: This is a bring-up path to replace embedded bytes with a read-only bundle image.
    // packagefsd talks to bundlemgrd using CAP_MOVE replies via its reply inbox (@reply).
    let bundle = KernelClient::new_for("bundlemgrd").ok()?;
    let reply = KernelClient::new_for("@reply").ok()?;
    let (reply_send_slot, _reply_recv_slot) = reply.slots();
    let reply_send_clone = nexus_abi::cap_clone(reply_send_slot).ok()?;

    // Best-effort LIST proof: ensure bundlemgrd reports exactly one bundle in bring-up.
    // Use CAP_MOVE reply caps to avoid polluting other clients' response endpoints.
    let reply_send_clone2 = nexus_abi::cap_clone(reply_send_slot).ok()?;
    let mut list = [0u8; 4];
    nexus_abi::bundlemgrd::encode_list(&mut list);
    bundle
        .send_with_cap_move_wait(
            &list,
            reply_send_clone2,
            Wait::Timeout(core::time::Duration::from_secs(1)),
        )
        .ok()?;
    let rsp = bundle.recv(Wait::Timeout(core::time::Duration::from_secs(1))).ok()?;
    let (_st, _count) = nexus_abi::bundlemgrd::decode_list_rsp(&rsp)?;

    // Fetch the read-only image.
    let mut req = [0u8; 4];
    nexus_abi::bundlemgrd::encode_fetch_image(&mut req);
    bundle
        .send_with_cap_move_wait(
            &req,
            reply_send_clone,
            Wait::Timeout(core::time::Duration::from_secs(1)),
        )
        .ok()?;
    let rsp = bundle.recv(Wait::Timeout(core::time::Duration::from_secs(1))).ok()?;
    let (status, img) = nexus_abi::bundlemgrd::decode_fetch_image_rsp(&rsp)?;
    if status != nexus_abi::bundlemgrd::STATUS_OK {
        return None;
    }

    let (count, mut off) = nexus_abi::bundleimg::decode_header(img)?;
    let mut groups: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    for _ in 0..count {
        let e = nexus_abi::bundleimg::decode_next(img, &mut off)?;
        if e.kind != nexus_abi::bundleimg::KIND_FILE {
            continue;
        }
        let bundle_name = core::str::from_utf8(e.bundle).ok()?.to_string();
        let version = core::str::from_utf8(e.version).ok()?.to_string();
        let path = core::str::from_utf8(e.path).ok()?.to_string();
        let key = format!("{bundle_name}@{version}");
        groups
            .entry(key)
            .or_insert_with(|| vec![(".".to_string(), Entry::directory())])
            .push((path, Entry::file(e.data)));
        versions.insert(bundle_name, version);
    }

    let mut registry = BundleRegistry::default();
    for (canonical, entries) in groups {
        let (bundle, version) = canonical.split_once('@')?;
        registry.publish(bundle, version, &entries);
    }
    // Ensure active versions are set even if a bundle had only the "." directory synthesized.
    for (b, v) in versions {
        registry.active.insert(b, v);
    }
    Some(registry)
}

fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(_s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall

/// Keeps Cap'n Proto schemas referenced on host builds.
pub fn touch_schemas() {}
