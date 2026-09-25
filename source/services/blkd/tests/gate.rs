// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the partition gate's whole matrix — every sender (the five storage services and
//! a stranger), every partition selector (and ones past the table), every op — against the
//! grants ADR-0044 and RFC-0089 §12.5 write down; everything not granted is denied.
//! OWNERS: @runtime

use blkd::gate::Gates;
use storage::blockproto::*;

const SENDERS: [&[u8]; 6] =
    [b"statefsd", b"vfsd", b"bootctld", b"updated", b"bundlemgrd", b"selftest-client"];
const OPS: [u8; 9] =
    [0, OP_INFO, OP_READ, OP_WRITE, OP_SYNC, OP_ARM_VMO, OP_READ_VMO, OP_RELEASE_VMO, 8];

fn reads(op: u8) -> bool {
    matches!(op, OP_INFO | OP_READ | OP_ARM_VMO | OP_READ_VMO | OP_RELEASE_VMO)
}

/// The grants as the contracts state them.
fn granted(sender: &[u8], part: u8, op: u8) -> bool {
    match (sender, part) {
        (b"statefsd", PART_STATE) | (b"vfsd", PART_DATA) | (b"bootctld", PART_BSB) => true,
        (b"updated", PART_BOOT_A | PART_BOOT_B | PART_SYSTEM_A | PART_SYSTEM_B) => true,
        (b"bundlemgrd", PART_SYSTEM_A | PART_SYSTEM_B) => reads(op),
        _ => false,
    }
}

#[test]
fn the_gate_is_exactly_the_granted_matrix() {
    let gates = Gates::system();
    let mut allowed = 0;
    for sender in SENDERS {
        let id = nexus_abi::service_id_from_name(sender);
        for part in 0..PART_COUNT + 3 {
            for op in OPS {
                let want = granted(sender, part, op);
                assert_eq!(
                    gates.allowed(id, part, op),
                    want,
                    "{} part={part} op={op}",
                    String::from_utf8_lossy(sender)
                );
                allowed += usize::from(want);
            }
        }
    }
    // 3 single-store owners + updated on four slots, all 9 op codes; bundlemgrd's 5 reads on 2.
    assert_eq!(allowed, (3 + 4) * OPS.len() + 2 * 5);
}

#[test]
fn test_reject_a_writer_on_a_store_it_does_not_own() {
    let gates = Gates::system();
    let id = nexus_abi::service_id_from_name;
    assert!(!gates.allowed(id(b"statefsd"), PART_DATA, OP_WRITE));
    assert!(!gates.allowed(id(b"vfsd"), PART_STATE, OP_WRITE));
    assert!(!gates.allowed(id(b"bundlemgrd"), PART_SYSTEM_A, OP_WRITE));
    assert!(!gates.allowed(id(b"bundlemgrd"), PART_SYSTEM_B, OP_SYNC));
    assert!(!gates.allowed(id(b"updated"), PART_BSB, OP_WRITE));
}

#[test]
fn test_reject_a_stranger_and_a_partition_past_the_table() {
    let gates = Gates::system();
    let stranger = nexus_abi::service_id_from_name(b"selftest-client");
    for part in 0..PART_COUNT {
        assert!(!gates.allowed(stranger, part, OP_READ), "part={part}");
    }
    let statefsd = nexus_abi::service_id_from_name(b"statefsd");
    assert!(!gates.allowed(statefsd, PART_COUNT, OP_READ));
    assert!(!gates.allowed(statefsd, u8::MAX, OP_READ));
}
