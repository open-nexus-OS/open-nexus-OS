//! CONTEXT: samgrd deterministic loopback tests
//! INTENT: Validate register/resolve flows with stable inputs
//! IDL (target): register(name, endpoint) → resolve(name)
//! DEPS: samgrd (service integration)
//! READINESS: Host backend ready; loopback transport established
//! TESTS: Register/resolve ok, resolve unknown returns not-found
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg(nexus_env = "host")]

use std::io::Cursor;
use std::thread;

use capnp::message::Builder;
use capnp::serialize;
use nexus_e2e::{call, samgr_loopback};
use nexus_idl_runtime::samgr_capnp::{
    register_request, register_response, resolve_request, resolve_response,
};

const OPCODE_REGISTER: u8 = 1;
const OPCODE_RESOLVE: u8 = 2;

#[test]
fn register_resolve_roundtrip() {
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
fn resolve_unknown_returns_not_found() {
    let (client, mut server) = samgr_loopback();
    let handle = thread::spawn(move || samgrd::run_with_transport(&mut server).unwrap());

    let resolve = build_resolve_frame("does.not.exist");
    let response = call(&client, resolve);
    let (found, endpoint) = parse_resolve(&response);
    assert!(!found, "unknown service should not be resolved");
    assert_eq!(endpoint, 0);

    drop(client);
    handle.join().expect("samgrd thread exits cleanly");
}

fn build_register_frame(name: &str, endpoint: u32) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<register_request::Builder<'_>>();
        req.set_name(name);
        req.set_endpoint(endpoint);
    }
    encode_frame(OPCODE_REGISTER, &message)
}

fn build_resolve_frame(name: &str) -> Vec<u8> {
    let mut message = Builder::new_default();
    {
        let mut req = message.init_root::<resolve_request::Builder<'_>>();
        req.set_name(name);
    }
    encode_frame(OPCODE_RESOLVE, &message)
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
    assert_eq!(frame.first(), Some(&OPCODE_REGISTER));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read register response");
    let response =
        message.get_root::<register_response::Reader<'_>>().expect("register response root");
    assert!(response.get_ok(), "register should succeed");
}

fn parse_resolve(frame: &[u8]) -> (bool, u32) {
    assert_eq!(frame.first(), Some(&OPCODE_RESOLVE));
    let mut cursor = Cursor::new(&frame[1..]);
    let message = serialize::read_message(&mut cursor, capnp::message::ReaderOptions::new())
        .expect("read resolve response");
    let response =
        message.get_root::<resolve_response::Reader<'_>>().expect("resolve response root");
    (response.get_found(), response.get_endpoint())
}
