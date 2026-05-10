// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for DSoftBus host transport (handshake + ping/pong)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 integration tests
//!
//! TEST_SCOPE:
//!   - Noise XK handshake over host transport (TCP loopback)
//!   - Encrypted stream framing and a ping/pong roundtrip
//!   - Deterministic auth-failure case (server static mismatch)
//!   - RFC-0008 Phase 1b: Identity binding enforcement (device_id mismatch)
//!
//! TEST_SCENARIOS:
//!   - handshake_happy_path_and_ping_pong_deterministic(): handshake succeeds and ping/pong completes
//!   - auth_failure_deterministic_server_static_mismatch(): handshake fails on mismatched server static
//!   - test_reject_identity_mismatch(): verifies session is rejected when device_id doesn't match noise_static_pub
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

use dsoftbus::{Announcement, Authenticator, HostAuthenticator, Session, Stream};
use identity::Identity;

fn recv_with_deadline<S: Stream>(
    stream: &mut S,
    deadline: Instant,
) -> Result<dsoftbus::FramePayload, dsoftbus::StreamError> {
    loop {
        if Instant::now() > deadline {
            return Err(dsoftbus::StreamError::Protocol("timed out waiting for frame".into()));
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
        HostAuthenticator::bind(server_addr, server_identity.clone()).expect("bind server");
    let server_port = server_auth.local_addr().port();

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
        HostAuthenticator::bind(server_addr, server_identity.clone()).expect("bind server");
    let server_port = server_auth.local_addr().port();

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
    let client_auth = HostAuthenticator::bind(client_addr, client_identity).expect("bind client");

    let err = match client_auth.connect(&bad_announcement) {
        Ok(_) => panic!("expected connect to fail"),
        Err(err) => err,
    };
    match err {
        dsoftbus::AuthError::Noise(_) | dsoftbus::AuthError::Io(_) => {}
        other => panic!("expected connect to fail with Noise/Io error, got: {other}"),
    }

    server_thread.join().expect("server thread join");
}

/// RFC-0008 Phase 1b: Verify that session is rejected when device_id doesn't match
/// the noise_static_pub binding. This simulates a malicious peer claiming to be
/// "device A" but using "device B's" static key.
///
/// In a real attack scenario:
/// 1. Attacker observes device A's announcement (device_id_A, noise_static_A)
/// 2. Attacker starts a server with device B's identity but claims device_id_A
/// 3. Client connects expecting device A's static key but gets device B's
/// 4. Handshake should fail because static key doesn't match what client expects
///
/// This is the security invariant that protects against identity spoofing:
/// "DON'T allow 'warn and continue' on identity verification failure"
#[test]
fn test_reject_identity_mismatch() {
    // Create two distinct identities: attacker and legitimate peer
    let legitimate_identity = Identity::generate().expect("legitimate identity");
    let attacker_identity = Identity::generate().expect("attacker identity");

    // Attacker binds with their own identity but will claim to be the legitimate peer
    let attacker_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let attacker_auth =
        HostAuthenticator::bind(attacker_addr, attacker_identity.clone()).expect("bind attacker");
    let attacker_port = attacker_auth.local_addr().port();

    // Create a spoofed announcement: claims legitimate_identity's device_id but
    // has attacker's noise_static_pub. This is the identity mismatch case.
    let spoofed_announcement = Announcement::new(
        legitimate_identity.device_id().clone(), // Attacker claims to be legitimate peer
        vec!["dsoftbusd".to_string()],
        attacker_port,
        attacker_auth.local_noise_public(), // But uses their own static key
    );

    // The key insight: client expects legitimate peer's static key (from a prior
    // legitimate discovery) but gets attacker's key. The handshake fails because
    // the Noise XK pattern requires the initiator to know the responder's static
    // key in advance (from announcement/discovery), and the wrong key will cause
    // decryption failures.

    let attacker_thread = thread::spawn(move || {
        // Attacker waits for connection; handshake will fail
        let _ = attacker_auth.accept();
    });

    // Client has the spoofed announcement and tries to connect
    let client_identity = Identity::generate().expect("client identity");
    let client_addr = SocketAddr::from(([127, 0, 0, 1], 0));
    let client_auth = HostAuthenticator::bind(client_addr, client_identity).expect("bind client");

    // The client uses the spoofed announcement which has the wrong static key
    // for the claimed device_id. Since Noise XK requires knowing the responder's
    // static key, this should fail.
    let result = client_auth.connect(&spoofed_announcement);

    // Connection should fail because the static key in the announcement doesn't
    // match what the server (attacker) actually has as its static key.
    // NOTE: In this test, the announcement has attacker's actual key, so Noise
    // handshake succeeds cryptographically. The identity mismatch detection
    // happens at a higher layer where we verify device_id -> noise_static mapping.
    //
    // For now, we just verify the handshake mechanics work. A full identity
    // binding implementation would cache legitimate (device_id, noise_static)
    // pairs from discovery and reject any session where the mapping doesn't match.

    // RFC-0008 Phase 1b: identity mismatch MUST be a hard reject at the API boundary.
    let err = match result {
        Ok(_) => panic!("expected identity mismatch to be rejected"),
        Err(err) => err,
    };
    match err {
        dsoftbus::AuthError::Identity(_) => {}
        other => panic!("expected AuthError::Identity, got: {other}"),
    }

    attacker_thread.join().expect("attacker thread join");
}
