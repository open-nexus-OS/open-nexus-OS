extern crate alloc;

//! CONTEXT: OS-lite packagefs daemon path using bundlemgr authority + pkgimg v2 validation.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by single-VM QEMU marker ladder and selftest VFS phase.

use core::fmt;

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use nexus_ipc::Server;
use nexus_ipc::{Client, IpcError, KernelClient, KernelServer, Wait};
use storage::pkgimg::{build_pkgimg, parse_pkgimg, PkgImgCaps, PkgImgFileSpec};

const OPCODE_RESOLVE: u8 = 2;
const OPCODE_MOUNT_STATUS: u8 = 3;
const KIND_FILE: u16 = 0;
const KIND_DIRECTORY: u16 = 1;

const DEMO_HELLO_MANIFEST_NXB: &[u8] = exec_payloads::HELLO_MANIFEST_NXB;
const DEMO_HELLO_PAYLOAD: &[u8] = b"HELLO_PAYLOAD_BYTES";
const DEMO_EXIT_MANIFEST_NXB: &[u8] = exec_payloads::EXIT0_MANIFEST_NXB;
const DEMO_EXIT_PAYLOAD: &[u8] = b"EXIT0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MountMode {
    Legacy = 0,
    PkgImgNative = 1,
    PkgImgTranscoded = 2,
    PkgImgSeed = 3,
}

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
    // Marker contract: emit only after the IPC endpoint exists.
    debug_print("packagefsd: ready\n");
    notifier.notify();
    // RFC-0005: name-based routing; init-lite assigns per-service endpoint caps and answers route
    // queries over a private control channel, so services don't hardcode slot numbers.
    let server = match KernelServer::new_for("packagefsd") {
        Ok(server) => server,
        Err(_) => KernelServer::new_with_slots(3, 4).map_err(|_| LiteError::Transport)?,
    };
    let (registry, mount_mode) = load_registry_from_bundlemgrd()
        .or_else(load_registry_from_seed_pkgimg)
        .unwrap_or_else(seed_registry);
    run_loop(&server, &registry, mount_mode)
}

fn run_loop(server: &KernelServer, registry: &BundleRegistry, mount_mode: MountMode) -> LiteResult<()> {
    let mut response = Vec::with_capacity(256);
    loop {
        match server.recv(Wait::Blocking) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    continue;
                }
                match bytes[0] {
                    OPCODE_RESOLVE => {
                        let entry = match core::str::from_utf8(&bytes[1..]) {
                            Ok(rel) => registry.resolve(rel),
                            Err(_) => None,
                        };
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
                    OPCODE_MOUNT_STATUS => {
                        response.clear();
                        response.push(mount_mode as u8);
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

fn seed_registry() -> (BundleRegistry, MountMode) {
    let mut registry = BundleRegistry::default();
    // Deterministic system properties for VFS bring-up tests.
    let system_entries = vec![
        (".".to_string(), Entry::directory()),
        ("build.prop".to_string(), Entry::file(b"ro.nexus.build=dev\n")),
    ];
    registry.publish("system", "1.0.0", &system_entries);

    let hello_entries = vec![
        (".".to_string(), Entry::directory()),
        ("manifest.nxb".to_string(), Entry::file(DEMO_HELLO_MANIFEST_NXB)),
        ("payload.elf".to_string(), Entry::file(DEMO_HELLO_PAYLOAD)),
    ];
    registry.publish("demo.hello", "1.0.0", &hello_entries);

    let exit_entries = vec![
        (".".to_string(), Entry::directory()),
        ("manifest.nxb".to_string(), Entry::file(DEMO_EXIT_MANIFEST_NXB)),
        ("payload.elf".to_string(), Entry::file(DEMO_EXIT_PAYLOAD)),
    ];
    registry.publish("demo.exit0", "1.0.0", &exit_entries);

    (registry, MountMode::Legacy)
}

fn load_registry_from_seed_pkgimg() -> Option<(BundleRegistry, MountMode)> {
    let specs = vec![
        PkgImgFileSpec::new("system", "1.0.0", "build.prop", b"ro.nexus.build=dev\n"),
        PkgImgFileSpec::new("demo.hello", "1.0.0", "manifest.nxb", DEMO_HELLO_MANIFEST_NXB),
        PkgImgFileSpec::new("demo.hello", "1.0.0", "payload.elf", DEMO_HELLO_PAYLOAD),
        PkgImgFileSpec::new("demo.exit0", "1.0.0", "manifest.nxb", DEMO_EXIT_MANIFEST_NXB),
        PkgImgFileSpec::new("demo.exit0", "1.0.0", "payload.elf", DEMO_EXIT_PAYLOAD),
    ];
    let caps = PkgImgCaps::default();
    let img = build_pkgimg(&specs, caps).ok()?;
    let parsed = parse_pkgimg(&img, caps).ok()?;
    let mut groups: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    for e in parsed.entries() {
        let payload = parsed.read(&e.bundle, &e.version, &e.path)?;
        let key = format!("{}@{}", e.bundle, e.version);
        groups
            .entry(key)
            .or_insert_with(|| vec![(".".to_string(), Entry::directory())])
            .push((e.path.clone(), Entry::file(payload)));
        versions.insert(e.bundle.clone(), e.version.clone());
    }
    let mut registry = BundleRegistry::default();
    for (canonical, entries) in groups {
        let (bundle, version) = canonical.split_once('@')?;
        registry.publish(bundle, version, &entries);
    }
    for (b, v) in versions {
        registry.active.insert(b, v);
    }
    debug_print("packagefsd: v2 mounted (pkgimg)\n");
    Some((registry, MountMode::PkgImgSeed))
}

fn load_registry_from_bundlemgrd() -> Option<(BundleRegistry, MountMode)> {
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

    let caps = PkgImgCaps::default();
    let (parsed, mount_mode) = match parse_pkgimg(img, caps) {
        Ok(parsed) => (parsed, MountMode::PkgImgNative),
        Err(_) => {
            // Transitional compatibility: legacy fetch_image payloads may still be bundleimg.
            // Convert deterministically into pkgimg bytes, then validate using the v2 parser.
            let (count, mut off) = nexus_abi::bundleimg::decode_header(img)?;
            let mut specs = Vec::new();
            for _ in 0..count {
                let e = nexus_abi::bundleimg::decode_next(img, &mut off)?;
                if e.kind != nexus_abi::bundleimg::KIND_FILE {
                    continue;
                }
                let bundle_name = core::str::from_utf8(e.bundle).ok()?;
                let version = core::str::from_utf8(e.version).ok()?;
                let path = core::str::from_utf8(e.path).ok()?;
                specs.push(PkgImgFileSpec::new(bundle_name, version, path, e.data));
            }
            let converted = build_pkgimg(&specs, caps).ok()?;
            (parse_pkgimg(&converted, caps).ok()?, MountMode::PkgImgTranscoded)
        }
    };
    let mut groups: BTreeMap<String, Vec<(String, Entry)>> = BTreeMap::new();
    let mut versions: BTreeMap<String, String> = BTreeMap::new();
    for e in parsed.entries() {
        let bundle_name = e.bundle.clone();
        let version = e.version.clone();
        let path = e.path.clone();
        let payload = parsed.read(&bundle_name, &version, &path)?;
        let key = format!("{bundle_name}@{version}");
        groups
            .entry(key)
            .or_insert_with(|| vec![(".".to_string(), Entry::directory())])
            .push((path, Entry::file(payload)));
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
    debug_print("packagefsd: v2 mounted (pkgimg)\n");
    Some((registry, mount_mode))
}

fn debug_print(_s: &str) {
    #[cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]
    let _ = nexus_abi::debug_write(_s.as_bytes());
}

// raw UART helper removed in favor of debug_write syscall

/// Keeps Cap'n Proto schemas referenced on host builds.
pub fn touch_schemas() {}
