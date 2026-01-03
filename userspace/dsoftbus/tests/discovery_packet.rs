// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Golden vector tests for DSoftBus discovery announce packet v1
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Stable
//! TEST_COVERAGE: 5 integration tests
//!
//! TEST_SCOPE:
//!   - Encode/decode determinism for discovery announce packet v1
//!   - Bounds enforcement (length caps)
//!
//! TEST_SCENARIOS:
//!   - announce_v1_golden_vector_bytes(): bytes are stable and exact
//!   - announce_v1_rejects_overlong_service_name(): bounds are enforced
//!   - announce_v1_rejects_bad_magic(): parser rejects incorrect magic
//!   - announce_v1_rejects_unsupported_version(): parser rejects incorrect version
//!   - announce_v1_rejects_truncated(): parser rejects truncated input
//!
//! DEPENDENCIES:
//!   - dsoftbus::discovery_packet: versioned packet encoder/decoder
//!
//! ADR: docs/adr/0005-dsoftbus-architecture.md

#[cfg(nexus_env = "host")]
mod host {
    use dsoftbus::discovery_packet::{
        decode_announce_v1, encode_announce_v1, AnnounceV1, PacketError,
    };

    #[test]
    fn announce_v1_golden_vector_bytes() {
        let pkt = AnnounceV1 {
            device_id: "node9".to_string(),
            port: 37020,
            noise_static: [0x11; 32],
            services: vec!["samgrd".into(), "bundlemgrd".into()],
        };

        let bytes = encode_announce_v1(&pkt).expect("encode");

        // MAGIC "NXSB", ver=1, dev_len=5, "node9", port=37020 (0x909C), noise_static, svc_count=2,
        // "samgrd", "bundlemgrd"
        let mut expected = Vec::new();
        expected.extend_from_slice(b"NXSB");
        expected.push(1);
        expected.push(5);
        expected.extend_from_slice(b"node9");
        expected.extend_from_slice(&37020u16.to_be_bytes());
        expected.extend_from_slice(&[0x11; 32]);
        expected.push(2);
        expected.push(5);
        expected.extend_from_slice(b"samgrd");
        expected.push(9);
        expected.extend_from_slice(b"bundlemgrd");

        assert_eq!(bytes, expected, "announce v1 bytes drifted");

        let decoded = decode_announce_v1(&bytes).expect("decode");
        assert_eq!(decoded, pkt);
    }

    #[test]
    fn announce_v1_rejects_overlong_service_name() {
        let pkt = AnnounceV1 {
            device_id: "node9".to_string(),
            port: 1,
            noise_static: [0; 32],
            services: vec!["a".repeat(100)],
        };
        let err = encode_announce_v1(&pkt).expect_err("expected bounds error");
        assert_eq!(err, PacketError::InvalidInput("service name length"));
    }

    #[test]
    fn announce_v1_rejects_bad_magic() {
        let bytes = b"NOPE";
        assert_eq!(decode_announce_v1(bytes), Err(PacketError::BadMagic));
    }

    #[test]
    fn announce_v1_rejects_unsupported_version() {
        // MAGIC + version=2 + dev_len=1 + "x" + port + noise + svc_count=0
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"NXSB");
        bytes.push(2);
        bytes.push(1);
        bytes.extend_from_slice(b"x");
        bytes.extend_from_slice(&1u16.to_be_bytes());
        bytes.extend_from_slice(&[0u8; 32]);
        bytes.push(0);
        assert_eq!(decode_announce_v1(&bytes), Err(PacketError::UnsupportedVersion(2)));
    }

    #[test]
    fn announce_v1_rejects_truncated() {
        let bytes = b"NXSB";
        assert_eq!(decode_announce_v1(bytes), Err(PacketError::Truncated));
    }
}

