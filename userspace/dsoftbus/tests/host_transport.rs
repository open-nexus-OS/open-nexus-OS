// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for DSoftBus host transport (handshake + ping/pong)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 integration tests
//!
//! TEST_SCOPE:
//!   - Noise XK handshake over host transport (TCP loopback)
//!   - Encrypted stream framing and a ping/pong roundtrip
//!   - Deterministic auth-failure case (server static mismatch)
//!
//! TEST_SCENARIOS:
//!   - handshake_happy_path_and_ping_pong_deterministic(): handshake succeeds and ping/pong completes
//!   - auth_failure_deterministic_server_static_mismatch(): handshake fails on mismatched server static
//!
//! DEPENDENCIES:
//!   - dsoftbus::{HostAuthenticator, Announcement}: host backend + discovery data
//!   - identity::Identity: device identity material for Noise proof
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#![cfg(nexus_env = "host")]

use std::net::SocketAddr;
use std::thread;
use std::time::{Duration, Instant};

use dsoftbus::{Announcement, Authenticator, InProcAuthenticator, Session, Stream};
use identity::Identity;

fn recv_with_deadline<S: Stream>(
    stream: &mut S,
    deadline: Instant,
) -> Result<dsoftbus::FramePayload, dsoftbus::StreamError> {
    loop {
        if Instant::now() > deadline {
            return Err(dsoftbus::StreamError::Protocol(
                "timed out waiting for frame".into(),
            ));
        }
        if let Some(frame) = stream.recv()? {
            return Ok(frame);
        }
        thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn handshake_happy_path_and_ping_pong_deterministic() {
    let server_identity = Identity::generate().expect("server identity");
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let server_auth =
        InProcAuthenticator::bind(server_addr, server_identity.clone()).expect("bind server");
    let server_port = server_auth.local_port();

    let announcement = Announcement::new(
        server_identity.device_id().clone(),
        vec!["dsoftbusd".to_string()],
        server_port,
        server_auth.local_noise_public(),
    );

    let server_thread = thread::spawn(move || {
        let session = server_auth.accept().expect("server accept");
        let mut stream = session.into_stream().expect("server into_stream");

        let deadline = Instant::now() + Duration::from_secs(2);
        let ping = recv_with_deadline(&mut stream, deadline).expect("server recv ping");
        assert_eq!(ping.channel, 1);
        assert_eq!(ping.bytes, b"ping".to_vec());

        stream.send(1, b"pong").expect("server send pong");
    });

    let client_identity = Identity::generate().expect("client identity");
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let client_auth = HostAuthenticator::bind(client_addr, client_identity).expect("bind client");

    let session = client_auth.connect(&announcement).expect("client connect");
    let mut stream = session.into_stream().expect("client into_stream");

    stream.send(1, b"ping").expect("client send ping");
    let deadline = Instant::now() + Duration::from_secs(2);
    let pong = recv_with_deadline(&mut stream, deadline).expect("client recv pong");
    assert_eq!(pong.channel, 1);
    assert_eq!(pong.bytes, b"pong".to_vec());

    server_thread.join().expect("server thread join");
}

#[test]
fn auth_failure_deterministic_server_static_mismatch() {
    let server_identity = Identity::generate().expect("server identity");
    let server_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let server_auth =
        InProcAuthenticator::bind(server_addr, server_identity.clone()).expect("bind server");
    let server_port = server_auth.local_port();

    let mut wrong_static = server_auth.local_noise_public();
    wrong_static[0] ^= 0x01;

    let bad_announcement = Announcement::new(
        server_identity.device_id().clone(),
        vec!["dsoftbusd".to_string()],
        server_port,
        wrong_static,
    );

    let server_thread = thread::spawn(move || {
        // The client will abort the handshake early; accept should fail deterministically.
        let _ = server_auth.accept();
    });

    let client_identity = Identity::generate().expect("client identity");
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let client_auth = InProcAuthenticator::bind(client_addr, client_identity).expect("bind client");

    let err = match client_auth.connect(&bad_announcement) {
        Ok(_) => panic!("expected connect to fail"),
        Err(err) => err,
    };
    let msg = err.to_string();
    assert!(
        msg.contains("server static mismatch"),
        "unexpected error message: {msg}"
    );

    server_thread.join().expect("server thread join");
}

