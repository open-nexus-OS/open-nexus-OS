extern crate alloc;

#[path = "../src/os/discovery/state.rs"]
mod discovery_state;
#[path = "../src/os/entry_pure.rs"]
mod entry_pure;
#[path = "../src/os/session/fsm.rs"]
mod fsm;
#[path = "../src/os/session/handshake.rs"]
mod handshake;
#[path = "../src/os/netstack/ids.rs"]
mod ids;
#[path = "../src/os/session/quic_frame.rs"]
mod quic_frame;

use alloc::string::String;
use alloc::vec::Vec;
use nexus_peer_lru::{PeerEntry, PeerLru};

#[test]
fn test_begin_reconnect_clears_sid_and_bumps_epoch() {
    let mut f: fsm::SessionFsm<u32> = fsm::SessionFsm::new();
    assert_eq!(f.epoch_raw(), 1);
    f.set_connected(7);
    assert_eq!(f.sid(), Some(7));

    let old = f.begin_reconnect();
    assert_eq!(old, Some(7));
    assert_eq!(f.sid(), None);
    assert_eq!(f.epoch_raw(), 2);
}

#[test]
fn test_set_phase_progression_smoke() {
    let mut f: fsm::SessionFsm<u32> = fsm::SessionFsm::new();
    assert_eq!(f.phase(), fsm::SessionPhase::Idle);
    f.set_listening();
    assert_eq!(f.phase(), fsm::SessionPhase::Listening);
    f.set_dialing();
    assert_eq!(f.phase(), fsm::SessionPhase::Dialing);
    f.set_accepting();
    assert_eq!(f.phase(), fsm::SessionPhase::Accepting);
    f.set_connected(42);
    assert_eq!(f.phase(), fsm::SessionPhase::Connected);
    assert_eq!(f.sid(), Some(42));
    f.set_handshaking();
    assert_eq!(f.phase(), fsm::SessionPhase::Handshaking);
    f.set_ready();
    assert_eq!(f.phase(), fsm::SessionPhase::Ready);
}

#[test]
fn test_set_peer_ip_insert_then_update() {
    let mut ips: Vec<(String, [u8; 4])> = Vec::new();
    discovery_state::set_peer_ip(&mut ips, "node-b", entry_pure::OS2VM_NODE_B_IP);
    discovery_state::set_peer_ip(&mut ips, "node-b", [10, 42, 0, 12]);
    assert_eq!(ips.len(), 1);
    assert_eq!(discovery_state::get_peer_ip(&ips, "node-b"), Some([10, 42, 0, 12]));
}

#[test]
fn test_get_peer_ip_missing_returns_none() {
    let ips: Vec<(String, [u8; 4])> = Vec::new();
    assert_eq!(discovery_state::get_peer_ip(&ips, "missing"), None);
    assert_eq!(discovery_state::DISC_PORT, 37_020);
    assert_eq!(discovery_state::MCAST_IP, [239, 42, 0, 1]);
}

#[test]
fn test_typed_id_roundtrip_raw() {
    let udp = ids::UdpSocketId::from_raw(7);
    let lid = ids::ListenerId::from_raw(8);
    let sid = ids::SessionId::from_raw(9);
    assert_eq!(udp.as_raw(), 7);
    assert_eq!(lid.as_raw(), 8);
    assert_eq!(sid.as_raw(), 9);
}

#[test]
fn test_derive_test_secret_deterministic_and_distinct() {
    let a = handshake::derive_test_secret(0xA0, 34_567);
    let b = handshake::derive_test_secret(0xA0, 34_567);
    let c = handshake::derive_test_secret(0xA1, 34_567);
    let d = handshake::derive_test_secret(0xA0, 34_568);
    assert_eq!(a, b);
    assert_ne!(a, c);
    assert_ne!(a, d);
}

#[test]
fn test_entry_pure_is_cross_vm_ip() {
    assert!(entry_pure::is_cross_vm_ip(entry_pure::OS2VM_NODE_A_IP));
    assert!(!entry_pure::is_cross_vm_ip([10, 42, 1, 10]));
    assert!(!entry_pure::is_cross_vm_ip(entry_pure::QEMU_USERNET_FALLBACK_IP));
}

#[test]
fn test_entry_pure_next_nonce_wraps() {
    let mut n = u64::MAX;
    let a = entry_pure::next_nonce(&mut n);
    let b = entry_pure::next_nonce(&mut n);
    assert_eq!(a, u64::MAX);
    assert_eq!(b, 0);
}

#[test]
fn test_entry_pure_rebuild_peer_ips_preserves_lru_order() {
    let mut peers = PeerLru::with_default_capacity();
    peers.insert(PeerEntry::new("node-a".into(), 1, [1; 32], alloc::vec![]));
    peers.insert(PeerEntry::new("node-b".into(), 2, [2; 32], alloc::vec![]));
    // Current order: node-b, node-a
    let mut ips = alloc::vec![
        ("node-a".into(), [10, 0, 0, 1]),
        ("node-b".into(), [10, 0, 0, 2]),
        ("stale".into(), [10, 0, 0, 99]),
    ];

    entry_pure::rebuild_peer_ips(&peers, &mut ips);
    assert_eq!(ips.len(), 2);
    assert_eq!(ips[0].0, "node-b");
    assert_eq!(ips[1].0, "node-a");
}

#[test]
fn test_entry_pure_set_get_and_derive_helpers() {
    let mut ips: Vec<(String, [u8; 4])> = Vec::new();
    entry_pure::set_peer_ip(&mut ips, "node-a", entry_pure::QEMU_USERNET_FALLBACK_IP);
    assert_eq!(entry_pure::get_peer_ip(&ips, "node-a"), Some(entry_pure::QEMU_USERNET_FALLBACK_IP));
    let a = entry_pure::derive_test_secret(0xD0, 34_567);
    let b = entry_pure::derive_test_secret(0xD0, 34_567);
    assert_eq!(a, b);
}

#[test]
fn test_quic_frame_roundtrip() {
    let mut out = [0u8; 256];
    let payload = b"PING";
    let encoded =
        quic_frame::encode_quic_frame(quic_frame::QUIC_OP_PING, 0x1122_3344, payload, &mut out)
            .expect("frame encodes");
    let decoded = quic_frame::decode_quic_frame(&out, encoded).expect("frame decodes");
    assert_eq!(decoded.0, quic_frame::QUIC_OP_PING);
    assert_eq!(decoded.1, 0x1122_3344);
    assert_eq!(decoded.2, payload);
}

#[test]
fn test_reject_quic_frame_bad_magic() {
    let mut out = [0u8; 256];
    let payload = b"PING";
    let encoded =
        quic_frame::encode_quic_frame(quic_frame::QUIC_OP_PING, 1, payload, &mut out).unwrap();
    out[0] = b'X';
    assert!(quic_frame::decode_quic_frame(&out, encoded).is_none());
}

#[test]
fn test_reject_quic_frame_truncated_payload() {
    let mut out = [0u8; 256];
    let payload = b"PING";
    let encoded =
        quic_frame::encode_quic_frame(quic_frame::QUIC_OP_PING, 1, payload, &mut out).unwrap();
    // Claim one extra payload byte without actually including it in n.
    out[8..10].copy_from_slice(&5u16.to_le_bytes());
    assert!(quic_frame::decode_quic_frame(&out, encoded).is_none());
}

#[test]
fn test_reject_quic_frame_oversized_payload_encode() {
    let mut out = [0u8; 256];
    let oversized = [0u8; 247];
    assert!(
        quic_frame::encode_quic_frame(quic_frame::QUIC_OP_PING, 1, &oversized, &mut out).is_none()
    );
}

#[test]
fn test_quic_frame_opcode_contract_values() {
    assert_eq!(quic_frame::QUIC_OP_MSG1, 1);
    assert_eq!(quic_frame::QUIC_OP_MSG2, 2);
    assert_eq!(quic_frame::QUIC_OP_MSG3, 3);
    assert_eq!(quic_frame::QUIC_OP_PING, 4);
    assert_eq!(quic_frame::QUIC_OP_PONG, 5);
}
