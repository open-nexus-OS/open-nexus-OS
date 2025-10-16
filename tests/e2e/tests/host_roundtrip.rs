// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(nexus_env = "host")]

use std::io::Cursor;
use std::thread;

use bundlemgrd::ArtifactStore;
use capnp::message::Builder;
use capnp::serialize;
use nexus_ipc::Client;
// use sha2::Digest;
use nexus_e2e::{bundle_loopback, call, samgr_loopback};
use nexus_idl_runtime::bundlemgr_capnp::{
    get_payload_request, get_payload_response, install_request, install_response, query_request,
    query_response, InstallError,
};
use nexus_idl_runtime::keystored_capnp::{device_id_request, device_id_response};
use nexus_idl_runtime::samgr_capnp::{
    register_request, register_response, resolve_request, resolve_response,
};

const SAMGR_OPCODE_REGISTER: u8 = 1;
const SAMGR_OPCODE_RESOLVE: u8 = 2;
const BUNDLE_OPCODE_INSTALL: u8 = 1;
const BUNDLE_OPCODE_QUERY: u8 = 2;
const BUNDLE_OPCODE_GET_PAYLOAD: u8 = 3;
const PUBLISHER: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SIG_HEX: &str =
    "11111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111";
fn valid_manifest_str() -> String {
    format!(
        "name = \"launcher\"\nversion = \"1.0.0\"\nabilities = [\"ui\"]\ncaps = [\"gpu\"]\nmin_sdk = \"0.1.0\"\npublisher = \"{}\"\nsig = \"{}\"\n",
        PUBLISHER, SIG_HEX
    )
}

#[test]
fn samgr_register_resolve_roundtrip() {
    let (client, mut server) = samgr_loopback();
    let handle = thread::spawn(move || samgrd::run_with_transport(&mut server).unwrap());

    let register = build_register_frame("shell", 7);
    let response = call(&client, register);
    assert_register_ok(&response);

    let resolve = build_resolve_frame("shell");
    let response = call(&client, resolve);
    let (found, endpoint) = parse_resolve(&response);
    assert!(found, "service should be resolved");
    assert_eq!(endpoint, 7);

    drop(client);
    handle.join().expect("samgrd thread exits cleanly");
}

#[test]
fn bundle_install_query_roundtrip() {
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let manifest = valid_manifest();
    let len = manifest.len() as u32;
    store.insert(42, manifest);
    store.stage_payload(42, vec![0xde, 0xad]);
    let store_clone = store.clone();
    let handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut server, store_clone, None).unwrap()
    });

    let install = build_install_frame("launcher", 42, len);
    let response = call(&client, install);
    let (ok, err) = parse_install(&response);
    assert!(ok, "install should succeed");
    assert_eq!(err, InstallError::None);

    let query = build_query_frame("launcher");
    let response = call(&client, query);
    let (installed, version, caps) = parse_query(&response);
    assert!(installed, "bundle should be installed");
    assert_eq!(version, "1.0.0");
    assert_eq!(caps, vec!["gpu".to_string()]);

    drop(client);
    handle.join().expect("bundlemgrd thread exits cleanly");
}

#[test]
fn bundle_install_get_payload_roundtrip() {
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let manifest = valid_manifest();
    let len = manifest.len() as u32;
    store.insert(99, manifest);
    let payload_bytes = vec![0xde, 0xad, 0xbe, 0xef];
    store.stage_payload(99, payload_bytes.clone());
    let store_clone = store.clone();

    let handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut server, store_clone, None).unwrap()
    });

    let install = build_install_frame("launcher", 99, len);
    let response = call(&client, install);
    let (ok, err) = parse_install(&response);
    assert!(ok, "install should succeed");
    assert_eq!(err, InstallError::None);

    let request = build_get_payload_frame("launcher");
    let response = call(&client, request);
    let (ok, bytes) = parse_get_payload(&response);
    assert!(ok, "payload should be returned");
    assert_eq!(bytes, payload_bytes);

    drop(client);
    handle.join().expect("bundlemgrd thread exits cleanly");
}

#[test]
fn bundle_install_invalid_signature() {
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let manifest = invalid_manifest();
    let len = manifest.len() as u32;
    store.insert(7, manifest);
    store.stage_payload(7, vec![0u8]);
    let store_clone = store.clone();

    let handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut server, store_clone, None).unwrap()
    });

    let install = build_install_frame("launcher", 7, len);
    let response = call(&client, install);
    let (ok, err) = parse_install(&response);
    assert!(!ok, "install should fail");
    assert_eq!(err, InstallError::InvalidSig);

    drop(client);
    handle.join().expect("bundlemgrd thread exits cleanly");
}

#[test]
fn bundle_install_signed_enforced_via_keystored() {
    // Prepare a temporary anchors directory and point keystored at it
    let tmp = tempfile::tempdir().expect("tempdir");
    std::env::set_var("NEXUS_ANCHORS_DIR", tmp.path());

    // Generate a new keypair and write public key as hex anchor
    use ed25519_dalek::{Signer, SigningKey};
    let sk = SigningKey::generate(&mut rand::rngs::OsRng);
    let pk = sk.verifying_key();
    let anchor_hex = hex::encode(pk.to_bytes());
    std::fs::write(tmp.path().join("dev.publisher.pub"), &anchor_hex).expect("write anchor");
    let publisher = keystore::device_id(&pk);

    // Start bundlemgrd with a keystore loopback wired
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let signed_payload = format!(
        "name = \"launcher\"\nversion = \"1.0.0\"\nabilities = [\"ui\"]\ncaps = [\"gpu\"]\nmin_sdk = \"0.1.0\"\npublisher = \"{}\"\n",
        publisher
    );
    let sig = sk.sign(signed_payload.as_bytes());
    let manifest = format!("{}sig = \"{}\"\n", signed_payload, hex::encode(sig.to_bytes()));
    let len = manifest.len() as u32;
    store.insert(77, manifest.into_bytes());
    store.stage_payload(77, vec![0xfa, 0xce, 0x00, 0x01]);
    let store_clone = store.clone();

    let handle = std::thread::spawn(move || {
        // Spawn keystored loopback with default anchors dir (overridden via env)
        let (ks_client, ks_server) = nexus_ipc::loopback_channel();
        std::thread::spawn(move || {
            let mut ks_transport = keystored::IpcTransport::new(ks_server);
            keystored::run_with_transport_default_anchors(&mut ks_transport).unwrap();
        });

        // Obtain device id
        let mut msg = capnp::message::Builder::new_default();
        {
            let _ = msg.init_root::<device_id_request::Builder<'_>>();
        }
        let mut buf = Vec::new();
        capnp::serialize::write_message(&mut buf, &msg).unwrap();
        let mut frame = Vec::with_capacity(1 + buf.len());
        frame.push(3u8);
        frame.extend_from_slice(&buf);
        ks_client.send(&frame, nexus_ipc::Wait::Blocking).unwrap();
        let resp = ks_client.recv(nexus_ipc::Wait::Blocking).unwrap();
        let mut cur = std::io::Cursor::new(&resp[1..]);
        let dev_msg =
            capnp::serialize::read_message(&mut cur, capnp::message::ReaderOptions::new()).unwrap();
        let dev = dev_msg.get_root::<device_id_response::Reader<'_>>().unwrap();
        let id = dev.get_id().unwrap().to_str().unwrap().to_string();

        // Build manifest by signing canonical content (no sig line) with keystore-provided id
        let signed_payload = format!(
            "name = \"launcher\"\nversion = \"1.0.0\"\nabilities = [\"ui\"]\ncaps = [\"gpu\"]\nmin_sdk = \"0.1.0\"\npublisher = \"{}\"\n",
            id
        );
        let sig = sk.sign(signed_payload.as_bytes());
        let manifest = format!("{}sig = \"{}\"\n", signed_payload, hex::encode(sig.to_bytes()));
        let len = manifest.len() as u32;
        store_clone.insert(77, manifest.into_bytes());
        store_clone.stage_payload(77, vec![0xfa, 0xce, 0x00, 0x02]);

        let keystore = Some(bundlemgrd::KeystoreHandle::from_loopback(ks_client));
        bundlemgrd::run_with_transport(&mut server, store_clone, keystore).unwrap()
    });

    let install = build_install_frame("launcher", 77, len);
    let response = call(&client, install);
    let (ok, err) = parse_install(&response);
    assert!(ok, "install should succeed with valid signature, err={:?}", err);

    drop(client);
    handle.join().expect("bundlemgrd thread exits cleanly");
}

fn build_register_frame(name: &str, endpoint: u32) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<register_request::Builder<'_>>();
        req.set_name(name);
        req.set_endpoint(endpoint);
    }
    encode_frame(SAMGR_OPCODE_REGISTER, &message)
}

fn build_resolve_frame(name: &str) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<resolve_request::Builder<'_>>();
        req.set_name(name);
    }
    encode_frame(SAMGR_OPCODE_RESOLVE, &message)
}

fn build_install_frame(name: &str, handle: u32, len: u32) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<install_request::Builder<'_>>();
        req.set_name(name);
        req.set_bytes_len(len);
        req.set_vmo_handle(handle);
    }
    encode_frame(BUNDLE_OPCODE_INSTALL, &message)
}

fn build_query_frame(name: &str) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<query_request::Builder<'_>>();
        req.set_name(name);
    }
    encode_frame(BUNDLE_OPCODE_QUERY, &message)
}

fn build_get_payload_frame(name: &str) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<get_payload_request::Builder<'_>>();
        req.set_name(name);
    }
    encode_frame(BUNDLE_OPCODE_GET_PAYLOAD, &message)
}

fn encode_frame(opcode: u8, message: &Builder<capnp::message::HeapAllocator>) -> Vec<u8> {
    let mut payload = Vec::new();
    serialize::write_message(&mut payload, message).expect("serialize frame");
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(opcode);
    frame.extend_from_slice(&payload);
    frame
}

fn assert_register_ok(frame: &[u8]) {
    assert_eq!(frame.first(), Some(&SAMGR_OPCODE_REGISTER));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read register response");
    let response =
        message.get_root::<register_response::Reader<'_>>().expect("register response root");
    assert!(response.get_ok(), "register should succeed");
}

fn parse_resolve(frame: &[u8]) -> (bool, u32) {
    assert_eq!(frame.first(), Some(&SAMGR_OPCODE_RESOLVE));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read resolve response");
    let response =
        message.get_root::<resolve_response::Reader<'_>>().expect("resolve response root");
    (response.get_found(), response.get_endpoint())
}

fn parse_install(frame: &[u8]) -> (bool, InstallError) {
    assert_eq!(frame.first(), Some(&BUNDLE_OPCODE_INSTALL));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read install response");
    let response =
        message.get_root::<install_response::Reader<'_>>().expect("install response root");
    (response.get_ok(), response.get_err().unwrap_or(InstallError::Einval))
}

fn parse_query(frame: &[u8]) -> (bool, String, Vec<String>) {
    assert_eq!(frame.first(), Some(&BUNDLE_OPCODE_QUERY));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read query response");
    let response = message.get_root::<query_response::Reader<'_>>().expect("query response root");
    let version =
        response.get_version().ok().and_then(|r| r.to_str().ok()).unwrap_or("").to_string();
    let mut caps = Vec::new();
    if let Ok(list) = response.get_required_caps() {
        for idx in 0..list.len() {
            if let Ok(cap) = list.get(idx) {
                if let Ok(text) = cap.to_str() {
                    caps.push(text.to_string());
                }
            }
        }
    }
    (response.get_installed(), version, caps)
}

fn parse_get_payload(frame: &[u8]) -> (bool, Vec<u8>) {
    assert_eq!(frame.first(), Some(&BUNDLE_OPCODE_GET_PAYLOAD));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read get_payload response");
    let response =
        message.get_root::<get_payload_response::Reader<'_>>().expect("get_payload response root");
    let ok = response.get_ok();
    let bytes = if ok {
        response.get_bytes().map(|data| data.to_vec()).unwrap_or_default()
    } else {
        Vec::new()
    };
    (ok, bytes)
}

fn valid_manifest() -> Vec<u8> {
    valid_manifest_str().into_bytes()
}

fn invalid_manifest() -> Vec<u8> {
    valid_manifest_str().replace(SIG_HEX, "00").into_bytes()
}
