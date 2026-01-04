//! CONTEXT: VFS end-to-end integration tests
//! INTENT: Package filesystem roundtrip through bundlemgrd/vfsd/packagefsd
//! IDL (target): installBundle(path), stat(path), open(path), read(fh,offset,len)
//! DEPS: bundlemgrd, vfsd, packagefsd (service integration)
//! READINESS: All services ready; loopback transport established
//! TESTS: Install bundle, VFS access, payload read, asset access, error handling
#![cfg(nexus_env = "host")]

use std::io::Cursor;
use std::sync::Arc;
use std::thread;

use bundlemgrd::{ArtifactStore, PackageFsHandle};
use capnp::message::ReaderOptions;
use capnp::serialize;
use nexus_idl_runtime::bundlemgr_capnp::{install_request, install_response};
use nexus_ipc::{Client, LoopbackClient, Wait};
use nexus_packagefs::PackageFsClient;
use nexus_vfs::{Error as VfsError, VfsClient};

const OPCODE_INSTALL: u8 = 1;
const MANIFEST_TOML: &str = "name = \"demo.hello\"\nversion = \"1.0.0\"\nabilities = [\"ui\"]\ncaps = [\"gpu\"]\nmin_sdk = \"0.1.0\"\npublisher = \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"\nsig = \"11111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111\"\n";
const PAYLOAD_BYTES: &[u8] = b"payload-bytes";
const LOGO_SVG: &[u8] = b"<svg/>";

#[test]
fn vfs_package_roundtrip() {
    let (pkg_client, mut pkg_server) = packagefsd::loopback_transport();
    let registry = packagefsd::BundleRegistry::global().clone();
    let packagefs_thread = thread::spawn(move || {
        packagefsd::run_with_transport(&mut pkg_server, registry).unwrap();
    });

    let packagefs_client = Arc::new(PackageFsClient::from_loopback(pkg_client));

    let (vfs_client_conn, mut vfs_server) = vfsd::loopback_transport();
    let vfs_packagefs = packagefs_client.clone();
    let vfs_thread = thread::spawn(move || {
        vfsd::run_with_transport(&mut vfs_server, vfs_packagefs).unwrap();
    });

    let (bundle_client, mut bundle_server) = bundlemgrd::loopback_transport();
    let artifacts = ArtifactStore::new();
    let store_clone = artifacts.clone();
    let packagefs_handle = PackageFsHandle::from_client(packagefs_client.clone());
    let bundle_thread = thread::spawn(move || {
        bundlemgrd::run_with_transport(
            &mut bundle_server,
            store_clone,
            None,
            Some(packagefs_handle),
        )
        .unwrap();
    });

    let handle = 42u32;
    let manifest_bytes = MANIFEST_TOML.as_bytes().to_vec();
    artifacts.insert(handle, manifest_bytes.clone());
    artifacts.stage_payload(handle, PAYLOAD_BYTES.to_vec());
    artifacts.stage_asset(handle, "assets/logo.svg", LOGO_SVG.to_vec());

    let install_frame = build_install_frame("demo.hello", handle, manifest_bytes.len() as u32);
    send_frame(&bundle_client, install_frame);
    let response = recv_frame(&bundle_client);
    assert_install_ok(&response);

    let vfs_client = VfsClient::from_loopback(vfs_client_conn);

    let meta = vfs_client.stat("pkg:/demo.hello/manifest.toml").expect("manifest stat succeeds");
    assert_eq!(meta.size(), manifest_bytes.len() as u64);

    let canonical = vfs_client
        .stat("/packages/demo.hello@1.0.0/manifest.toml")
        .expect("canonical stat succeeds");
    assert_eq!(canonical.size(), meta.size());

    let fh = vfs_client.open("pkg:/demo.hello/payload.elf").expect("open payload succeeds");
    let payload = vfs_client.read(fh, 0, PAYLOAD_BYTES.len()).expect("read payload succeeds");
    assert_eq!(payload, PAYLOAD_BYTES);

    // Out-of-range reads must clamp to the available tail
    let start = PAYLOAD_BYTES.len().saturating_sub(2) as u64;
    let clamped = vfs_client.read(fh, start, 10).expect("read beyond end clamps");
    assert_eq!(clamped, &PAYLOAD_BYTES[PAYLOAD_BYTES.len().saturating_sub(2)..]);

    // Reads starting past EOF should return an empty slice
    let empty = vfs_client
        .read(fh, PAYLOAD_BYTES.len() as u64 + 16, 32)
        .expect("read past EOF yields empty slice");
    assert!(empty.is_empty());
    vfs_client.close(fh).expect("close succeeds");
    assert_eq!(vfs_client.read(fh, 0, 1).unwrap_err(), VfsError::InvalidHandle);

    let asset_meta =
        vfs_client.stat("pkg:/demo.hello/assets/logo.svg").expect("asset stat succeeds");
    assert_eq!(asset_meta.size(), LOGO_SVG.len() as u64);

    assert_eq!(vfs_client.open("pkg:/demo.hello/missing.txt").unwrap_err(), VfsError::NotFound);

    // Drop clients in order to allow servers to observe disconnect and exit
    drop(vfs_client);
    drop(bundle_client);
    bundle_thread.join().expect("bundlemgrd exits cleanly");
    vfs_thread.join().expect("vfsd exits cleanly");
    drop(packagefs_client);
    packagefs_thread.join().expect("packagefsd exits cleanly");
}

fn build_install_frame(name: &str, handle: u32, manifest_len: u32) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut request = message.init_root::<install_request::Builder<'_>>();
        request.set_name(name);
        request.set_bytes_len(manifest_len);
        request.set_vmo_handle(handle);
    }
    let mut body = Vec::new();
    serialize::write_message(&mut body, &message).unwrap();
    let mut frame = Vec::with_capacity(1 + body.len());
    frame.push(OPCODE_INSTALL);
    frame.extend_from_slice(&body);
    frame
}

fn send_frame(client: &LoopbackClient, frame: Vec<u8>) {
    client.send(&frame, Wait::Blocking).expect("send frame");
}

fn recv_frame(client: &LoopbackClient) -> Vec<u8> {
    client.recv(Wait::Blocking).expect("recv frame")
}

fn assert_install_ok(response: &[u8]) {
    let (opcode, payload) = response.split_first().expect("non-empty install response");
    assert_eq!(*opcode, OPCODE_INSTALL);
    let mut cursor = Cursor::new(payload);
    let message = serialize::read_message(&mut cursor, ReaderOptions::new()).unwrap();
    let reader = message.get_root::<install_response::Reader<'_>>().unwrap();
    assert!(reader.get_ok(), "install should succeed");
}
