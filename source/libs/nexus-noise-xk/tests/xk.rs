// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host regression tests for no_std Noise XK core (happy path + mismatch + bounds)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 tests
//! ADR: docs/adr/0005-dsoftbus-architecture.md

use nexus_noise_xk::{
    NoiseError, StaticKeypair, Transport, XkInitiator, XkResponder, MSG1_LEN, MSG2_LEN, MSG3_LEN,
    TAGLEN,
};

fn keypair(secret_byte: u8) -> StaticKeypair {
    StaticKeypair::from_secret([secret_byte; 32])
}

#[test]
fn xk_happy_path_roundtrip_encrypt_decrypt() {
    let client_static = keypair(0x11);
    let server_static = keypair(0x22);

    let mut initiator = XkInitiator::new(client_static, server_static.public, [0xA1; 32]);
    let mut responder = XkResponder::new(server_static, client_static.public, [0xB2; 32]);

    let mut msg1 = [0u8; MSG1_LEN];
    initiator.write_msg1(&mut msg1);

    let mut msg2 = [0u8; MSG2_LEN];
    responder.read_msg1_write_msg2(&msg1, &mut msg2).unwrap();

    let mut msg3 = [0u8; MSG3_LEN];
    let i_keys = initiator.read_msg2_write_msg3(&msg2, &mut msg3).unwrap();
    let r_keys = responder.read_msg3_finish(&msg3).unwrap();

    let mut i = Transport::new(i_keys);
    let mut r = Transport::new(r_keys);

    let mut ct = [0u8; 4 + TAGLEN];
    assert_eq!(i.encrypt(b"PING", &mut ct).unwrap(), ct.len());

    let mut pt = [0u8; 4];
    assert_eq!(r.decrypt(&ct, &mut pt).unwrap(), pt.len());
    assert_eq!(&pt, b"PING");
}

#[test]
fn xk_fails_on_pinned_static_mismatch() {
    let client_static = keypair(0x33);
    let server_static = keypair(0x44);

    // Responder expects a different client static pub key.
    let wrong_client_pub = keypair(0x55).public;
    let mut responder = XkResponder::new(server_static, wrong_client_pub, [0x01; 32]);

    let mut initiator = XkInitiator::new(client_static, server_static.public, [0x02; 32]);

    let mut msg1 = [0u8; MSG1_LEN];
    initiator.write_msg1(&mut msg1);

    let mut msg2 = [0u8; MSG2_LEN];
    responder.read_msg1_write_msg2(&msg1, &mut msg2).unwrap();

    let mut msg3 = [0u8; MSG3_LEN];
    let _ = initiator.read_msg2_write_msg3(&msg2, &mut msg3).unwrap();

    let err = responder.read_msg3_finish(&msg3).unwrap_err();
    assert_eq!(err, NoiseError::StaticKeyMismatch);
}

#[test]
fn xk_rejects_bad_lengths_deterministically() {
    let client_static = keypair(0x66);
    let server_static = keypair(0x77);

    let mut initiator = XkInitiator::new(client_static, server_static.public, [0x10; 32]);
    let mut responder = XkResponder::new(server_static, client_static.public, [0x20; 32]);

    let mut msg1 = [0u8; MSG1_LEN];
    initiator.write_msg1(&mut msg1);

    let mut msg2 = [0u8; MSG2_LEN];
    responder.read_msg1_write_msg2(&msg1, &mut msg2).unwrap();

    // Too short for msg2.
    let mut msg3 = [0u8; MSG3_LEN];
    let err = initiator.read_msg2_write_msg3(&msg2[..MSG2_LEN - 1], &mut msg3).unwrap_err();
    assert_eq!(err, NoiseError::BadLength);

    // Too short for msg1.
    let mut msg2b = [0u8; MSG2_LEN];
    let err = responder.read_msg1_write_msg2(&msg1[..MSG1_LEN - 1], &mut msg2b).unwrap_err();
    assert_eq!(err, NoiseError::BadLength);
}
