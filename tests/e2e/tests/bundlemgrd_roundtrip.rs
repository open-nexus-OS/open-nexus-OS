//! CONTEXT: bundlemgrd deterministic loopback tests
//! INTENT: Validate install/query/payload flows with fixed manifests
//! IDL (target): install(name,handle,len) → query(name) → getPayload(name)
//! DEPS: bundlemgrd (service integration)
//! READINESS: Host backend ready; loopback transport established
//! TESTS: Install/query ok, getPayload ok, invalid signature rejected
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(nexus_env = "host")]

use std::io::Cursor;
use std::thread;

use bundlemgrd::ArtifactStore;
use capnp::message::Builder;
use capnp::serialize;
use nexus_e2e::{bundle_loopback, call};
use nexus_idl_runtime::bundlemgr_capnp::{
    get_payload_request, get_payload_response, install_request, install_response, query_request,
    query_response, InstallError,
};
use nexus_idl_runtime::manifest_capnp::bundle_manifest;

const OPCODE_INSTALL: u8 = 1;
const OPCODE_QUERY: u8 = 2;
const OPCODE_GET_PAYLOAD: u8 = 3;
const PUBLISHER_HEX: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"; // 16 bytes (32 hex chars)

#[test]
fn install_query_roundtrip() {
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let manifest = valid_manifest();
    let len = manifest.len() as u32;
    store.insert(42, manifest);
    store.stage_payload(42, vec![0xde, 0xad]);
    let store_clone = store.clone();
    let handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut server, store_clone, None, None).unwrap()
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
fn install_get_payload_roundtrip() {
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let manifest = valid_manifest();
    let len = manifest.len() as u32;
    store.insert(99, manifest);
    let payload_bytes = vec![0xde, 0xad, 0xbe, 0xef];
    store.stage_payload(99, payload_bytes.clone());
    let store_clone = store.clone();
    let handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut server, store_clone, None, None).unwrap()
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
fn install_invalid_signature_rejected() {
    let (client, mut server) = bundle_loopback();
    let store = ArtifactStore::new();
    let manifest = invalid_manifest();
    let len = manifest.len() as u32;
    store.insert(7, manifest);
    store.stage_payload(7, vec![0u8]);
    let store_clone = store.clone();
    let handle = thread::spawn(move || {
        bundlemgrd::run_with_transport(&mut server, store_clone, None, None).unwrap()
    });

    let install = build_install_frame("launcher", 7, len);
    let response = call(&client, install);
    let (ok, err) = parse_install(&response);
    assert!(!ok, "install should fail");
    assert_eq!(err, InstallError::InvalidSig);

    drop(client);
    handle.join().expect("bundlemgrd thread exits cleanly");
}

fn build_install_frame(name: &str, handle: u32, len: u32) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<install_request::Builder<'_>>();
        req.set_name(name);
        req.set_bytes_len(len);
        req.set_vmo_handle(handle);
    }
    encode_frame(OPCODE_INSTALL, &message)
}

fn build_query_frame(name: &str) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<query_request::Builder<'_>>();
        req.set_name(name);
    }
    encode_frame(OPCODE_QUERY, &message)
}

fn build_get_payload_frame(name: &str) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<get_payload_request::Builder<'_>>();
        req.set_name(name);
    }
    encode_frame(OPCODE_GET_PAYLOAD, &message)
}

fn encode_frame(opcode: u8, message: &Builder<capnp::message::HeapAllocator>) -> Vec<u8> {
    let mut payload = Vec::new();
    serialize::write_message(&mut payload, message).expect("serialize frame");
    let mut frame = Vec::with_capacity(1 + payload.len());
    frame.push(opcode);
    frame.extend_from_slice(&payload);
    frame
}

fn parse_install(frame: &[u8]) -> (bool, InstallError) {
    assert_eq!(frame.first(), Some(&OPCODE_INSTALL));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read install response");
    let response =
        message.get_root::<install_response::Reader<'_>>().expect("install response root");
    (response.get_ok(), response.get_err().unwrap_or(InstallError::Einval))
}

fn parse_query(frame: &[u8]) -> (bool, String, Vec<String>) {
    assert_eq!(frame.first(), Some(&OPCODE_QUERY));
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
    assert_eq!(frame.first(), Some(&OPCODE_GET_PAYLOAD));
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
    build_manifest_nxb(
        "launcher",
        "1.0.0",
        &["ui"],
        &["gpu"],
        &hex::decode(PUBLISHER_HEX).expect("publisher hex").try_into().expect("16 bytes"),
        &[0x11; 64],
    )
}

fn invalid_manifest() -> Vec<u8> {
    // Invalid by schema validation: signature length != 64.
    let publisher: [u8; 16] =
        hex::decode(PUBLISHER_HEX).expect("publisher hex").try_into().expect("16 bytes");
    let mut message = Builder::new_default();
    {
        let mut m = message.init_root::<bundle_manifest::Builder<'_>>();
        m.set_schema_version(1);
        m.set_name("launcher");
        m.set_semver("1.0.0");
        m.set_min_sdk("0.1.0");
        m.set_publisher(&publisher);
        m.set_signature(&[0x00]); // invalid length
        let mut a = m.reborrow().init_abilities(1);
        a.set(0, "ui");
        let mut c = m.reborrow().init_capabilities(1);
        c.set(0, "gpu");
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &message).expect("serialize");
    out
}

fn build_manifest_nxb(
    name: &str,
    semver: &str,
    abilities: &[&str],
    caps: &[&str],
    publisher: &[u8; 16],
    signature: &[u8; 64],
) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut m = message.init_root::<bundle_manifest::Builder<'_>>();
        m.set_schema_version(1);
        m.set_name(name);
        m.set_semver(semver);
        m.set_min_sdk("0.1.0");
        m.set_publisher(publisher);
        m.set_signature(signature);
        let mut a = m.reborrow().init_abilities(abilities.len() as u32);
        for (i, ab) in abilities.iter().enumerate() {
            a.set(i as u32, ab);
        }
        let mut c = m.reborrow().init_capabilities(caps.len() as u32);
        for (i, cap) in caps.iter().enumerate() {
            c.set(i as u32, cap);
        }
    }
    let mut out = Vec::new();
    serialize::write_message(&mut out, &message).expect("serialize");
    out
}
