// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::io::Cursor;
use std::thread;

use bundlemgrd::ArtifactStore;
use capnp::message::Builder;
use capnp::serialize;
use nexus_e2e::{bundle_loopback, samgr_loopback};
use nexus_idl_runtime::bundlemgr_capnp::{
    install_error, install_request, install_response, query_request, query_response,
};
use nexus_idl_runtime::samgr_capnp::{
    register_request, register_response, resolve_request, resolve_response,
};

const SAMGR_OPCODE_REGISTER: u8 = 1;
const SAMGR_OPCODE_RESOLVE: u8 = 2;
const BUNDLE_OPCODE_INSTALL: u8 = 1;
const BUNDLE_OPCODE_QUERY: u8 = 2;
const VALID_MANIFEST: &str = r#"
name = "launcher"
version = "1.0.0"
abilities = ["ui"]
caps = ["gpu"]
min_sdk = "0.1.0"
signature = "valid"
"#;

#[test]
fn samgr_register_resolve_roundtrip() {
    let (client, mut server) = samgr_loopback();
    let handle = thread::spawn(move || samgrd::run_with_transport(&mut server).unwrap());

    let register = build_register_frame("shell", 7);
    let response = client.call(register);
    assert_register_ok(&response);

    let resolve = build_resolve_frame("shell");
    let response = client.call(resolve);
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
    let store_clone = store.clone();

    let handle =
        thread::spawn(move || bundlemgrd::run_with_transport(&mut server, store_clone).unwrap());

    let install = build_install_frame("launcher", 42, len);
    let response = client.call(install);
    let (ok, err) = parse_install(&response);
    assert!(ok, "install should succeed");
    assert_eq!(err, install_error::Type::None);

    let query = build_query_frame("launcher");
    let response = client.call(query);
    let (installed, version) = parse_query(&response);
    assert!(installed, "bundle should be installed");
    assert_eq!(version, "1.0.0");

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
    let store_clone = store.clone();

    let handle =
        thread::spawn(move || bundlemgrd::run_with_transport(&mut server, store_clone).unwrap());

    let install = build_install_frame("launcher", 7, len);
    let response = client.call(install);
    let (ok, err) = parse_install(&response);
    assert!(!ok, "install should fail");
    assert_eq!(err, install_error::Type::Eacces);

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
    let response = message
        .get_root::<register_response::Reader<'_>>()
        .expect("register response root");
    assert!(response.get_ok(), "register should succeed");
}

fn parse_resolve(frame: &[u8]) -> (bool, u32) {
    assert_eq!(frame.first(), Some(&SAMGR_OPCODE_RESOLVE));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read resolve response");
    let response = message
        .get_root::<resolve_response::Reader<'_>>()
        .expect("resolve response root");
    (response.get_found(), response.get_endpoint())
}

fn parse_install(frame: &[u8]) -> (bool, install_error::Type) {
    assert_eq!(frame.first(), Some(&BUNDLE_OPCODE_INSTALL));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read install response");
    let response = message
        .get_root::<install_response::Reader<'_>>()
        .expect("install response root");
    (response.get_ok(), response.get_err())
}

fn parse_query(frame: &[u8]) -> (bool, String) {
    assert_eq!(frame.first(), Some(&BUNDLE_OPCODE_QUERY));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read query response");
    let response = message
        .get_root::<query_response::Reader<'_>>()
        .expect("query response root");
    let version = response.get_version().unwrap_or("").to_string();
    (response.get_installed(), version)
}

fn valid_manifest() -> Vec<u8> {
    VALID_MANIFEST.as_bytes().to_vec()
}

fn invalid_manifest() -> Vec<u8> {
    VALID_MANIFEST.replace("valid", "invalid").into_bytes()
}
