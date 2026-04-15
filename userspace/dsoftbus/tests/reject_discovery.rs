// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Negative tests for discovery announce robustness (TASK-0004 / RFC-0007)
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 3 integration tests (malformed/oversized/replay)
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(nexus_env = "host")]
mod host {
    use std::net::SocketAddr;

    use dsoftbus::discovery_packet::{
        decode_announce_v1, encode_announce_v1, AnnounceV1, PacketError,
    };
    use dsoftbus::{Announcement, Discovery, FacadeDiscovery};
    use identity::Identity;
    use nexus_net::fake::FakeNet;

    #[test]
    fn test_reject_malformed_announce() {
        // Malformed payloads MUST be rejected deterministically (no crash, no partial parse).
        assert_eq!(decode_announce_v1(b"NOPE"), Err(PacketError::BadMagic));
    }

    #[test]
    fn test_reject_oversized_announce() {
        let pkt = AnnounceV1 {
            device_id: "x".repeat(1000),
            port: 1,
            noise_static: [0u8; 32],
            services: vec!["samgrd".into()],
        };
        let err = encode_announce_v1(&pkt).expect_err("expected bounds error");
        assert_eq!(err, PacketError::InvalidInput("device_id length"));
    }

    #[test]
    fn test_reject_replay_announce() {
        let net = FakeNet::new();
        let bus = SocketAddr::from(([127, 0, 0, 1], 37121));

        let identity_a = Identity::generate().expect("identity a");
        let ann_a = Announcement::new(
            identity_a.device_id().clone(),
            vec!["samgrd".to_string()],
            2222,
            [0x33; 32],
        );

        let disc_a = FacadeDiscovery::new(net.clone(), bus, bus).expect("disc a");
        let disc_w = FacadeDiscovery::new(net, bus, bus).expect("watcher");

        // Same announce twice => only one yield (replay dedup).
        disc_a.announce(ann_a.clone()).expect("announce 1");
        disc_a.announce(ann_a.clone()).expect("announce 2");

        let mut w = disc_w.watch().expect("watch");
        let first = w.next().expect("first");
        assert_eq!(first.device_id(), ann_a.device_id());
        assert!(w.next().is_none(), "replayed announce should be ignored (no second yield)");
    }
}
