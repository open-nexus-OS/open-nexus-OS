// Copyright 2024 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![forbid(unsafe_code)]

#[cfg(nexus_env = "host")]
use nexus_ipc::{Client, LoopbackClient, LoopbackServer, Wait};

/// Client helper that sends a request frame and waits for the reply.
#[cfg(nexus_env = "host")]
pub fn call(client: &LoopbackClient, frame: Vec<u8>) -> Vec<u8> {
    if let Err(err) = client.send(&frame, Wait::Blocking) {
        panic!("send frame: {err}");
    }
    match client.recv(Wait::Blocking) {
        Ok(bytes) => bytes,
        Err(err) => panic!("recv frame: {err}"),
    }
}

/// Creates a loopback transport pair for the SAMGR daemon.
#[cfg(nexus_env = "host")]
pub fn samgr_loopback() -> (LoopbackClient, samgrd::IpcTransport<LoopbackServer>) {
    samgrd::loopback_transport()
}

/// Creates a loopback transport pair for the bundle manager daemon.
#[cfg(nexus_env = "host")]
pub fn bundle_loopback() -> (LoopbackClient, bundlemgrd::IpcTransport<LoopbackServer>) {
    bundlemgrd::loopback_transport()
}
