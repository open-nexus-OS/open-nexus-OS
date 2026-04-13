// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Integration tests for DSoftBus over nexus-net sockets facade (FakeNet)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 2 integration tests
//!
//! TEST_SCOPE:
//!   - DSoftBus handshake + stream framing over the sockets facade contract (`nexus-net`)
//!   - Deterministic auth-failure case (server static mismatch)
//!
//! TEST_SCENARIOS:
//!   - facade_handshake_happy_path_and_ping_pong(): handshake succeeds and ping/pong completes
//!   - facade_auth_failure_server_static_mismatch(): connect fails deterministically
//!   - facade_accept_times_out_deterministically(): accept times out without connection (tick deadline)
//!
//! DEPENDENCIES:
//!   - dsoftbus::FacadeAuthenticator: DSoftBus transport over sockets facade
//!   - nexus_net::fake::FakeNet: deterministic in-memory TCP backend
//!   - identity::Identity: device identity material for Noise proof
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(nexus_env = "host")]
mod host {
    use std::net::SocketAddr;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use dsoftbus::{Announcement, FacadeAuthenticator, Session, Stream};
    use identity::Identity;
    use nexus_net::fake::FakeNet;

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
    fn facade_handshake_happy_path_and_ping_pong() {
        let net = FakeNet::new();

        let server_identity = Identity::generate().expect("server identity");
        let server_auth = FacadeAuthenticator::new(
            net.clone(),
            SocketAddr::from(([127, 0, 0, 1], 0)),
            server_identity.clone(),
        )
        .expect("bind server");
        let server_port = server_auth.local_port();

        let announcement = Announcement::new(
            server_identity.device_id().clone(),
            vec!["dsoftbusd".to_string()],
            server_port,
            server_auth.local_noise_public(),
        );

        // Determinism guard: avoid a race where the server drops the stream immediately after
        // sending "pong", and the client observes disconnect before it polls the queued frame.
        let (ack_tx, ack_rx) = mpsc::channel::<()>();

        let server_thread = thread::spawn(move || {
            let session = server_auth.accept().expect("server accept");
            let mut stream = session.into_stream().expect("server into_stream");

            let deadline = Instant::now() + Duration::from_secs(2);
            let ping = recv_with_deadline(&mut stream, deadline).expect("server recv ping");
            assert_eq!(ping.channel, 1);
            assert_eq!(ping.bytes, b"ping".to_vec());

            stream.send(1, b"pong").expect("server send pong");

            // Wait for the client to confirm it received pong before closing the stream.
            ack_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("server wait pong-ack");
        });

        let client_identity = Identity::generate().expect("client identity");
        let client_auth =
            FacadeAuthenticator::new(net, SocketAddr::from(([127, 0, 0, 1], 0)), client_identity)
                .expect("bind client");

        let session = client_auth.connect(&announcement).expect("client connect");
        let mut stream = session.into_stream().expect("client into_stream");

        stream.send(1, b"ping").expect("client send ping");
        let deadline = Instant::now() + Duration::from_secs(2);
        let pong = recv_with_deadline(&mut stream, deadline).expect("client recv pong");
        assert_eq!(pong.channel, 1);
        assert_eq!(pong.bytes, b"pong".to_vec());

        ack_tx.send(()).expect("client send pong-ack");
        server_thread.join().expect("server thread join");
    }

    #[test]
    fn facade_auth_failure_server_static_mismatch() {
        let net = FakeNet::new();

        let server_identity = Identity::generate().expect("server identity");
        let server_auth = FacadeAuthenticator::new(
            net.clone(),
            SocketAddr::from(([127, 0, 0, 1], 0)),
            server_identity.clone(),
        )
        .expect("bind server");
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
            let _ = server_auth.accept();
        });

        let client_identity = Identity::generate().expect("client identity");
        let client_auth =
            FacadeAuthenticator::new(net, SocketAddr::from(([127, 0, 0, 1], 0)), client_identity)
                .expect("bind client");

        let err = match client_auth.connect(&bad_announcement) {
            Ok(_) => panic!("expected connect to fail"),
            Err(err) => err,
        };
        // Noise XK pins the responder static key. If the announcement contains the wrong key,
        // the handshake must fail deterministically (either as a Noise failure or as an IO error
        // after the peer drops the connection).
        match err {
            dsoftbus::AuthError::Noise(_) | dsoftbus::AuthError::Io(_) => {}
            other => panic!("expected connect to fail with Noise/Io error, got: {other}"),
        }

        server_thread.join().expect("server thread join");
    }

    #[test]
    fn facade_accept_times_out_deterministically() {
        let net = FakeNet::new();
        let identity = Identity::generate().expect("identity");
        let auth = FacadeAuthenticator::new(net, SocketAddr::from(([127, 0, 0, 1], 0)), identity)
            .expect("bind");
        let err = match auth.accept() {
            Ok(_) => panic!("expected timeout"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("timed out"), "err={err}");
    }
}
