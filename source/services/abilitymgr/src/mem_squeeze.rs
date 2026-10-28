// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The memory authority's squeeze order (TASK-0333 6f-1, RFC-0100 §5, ADR-0087). At
//! `critical` memd asks for a frozen app's private memory to be squeezed before it asks for any
//! kill; this service holds the lifecycle truth: it refuses the order for an instance that is not
//! suspended (a running app is never squeezed on the authority's word — and the kernel would
//! refuse it anyway), finds the instance by the process the order names, and lets the OS loop
//! carry `OP_SQUEEZE` to the parent (execd, `nexus_wire::execd`), whose totals travel back up in
//! the reply. Taken from memd's kernel sender id alone. Pure; the OS loop owns the route.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (string-identical to memd's `order.rs`)
//! TEST_COVERAGE: tests below (`test_reject_mem_squeeze_from_anyone_but_memd`,
//!   `test_reject_mem_squeeze_of_an_app_that_is_not_frozen`, the frozen instance, malformed, the
//!   reply bytes)

use crate::lifecycle::{AbilityState, Broker};
use crate::protocol::{MAGIC0, MAGIC1, OP_MEM_SQUEEZE, OP_RESPONSE, VERSION};

/// What a squeeze took, as the parent counted it (the batches of `task_squeeze`, added up).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Squeezed {
    /// Pages recorded compressed.
    pub pages: u32,
    /// Pages of zeroes dropped.
    pub zero: u32,
    /// Committed pages kept as they are.
    pub kept: u32,
    /// Frames the stores took.
    pub taken: u32,
    /// Bytes of the records written.
    pub stored: u32,
}

/// What one `OP_MEM_SQUEEZE` frame came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemSqueezeOutcome {
    /// Not the 8-byte frame.
    Malformed,
    /// No live instance runs as that process.
    Unknown { pid: u32 },
    /// The instance is not suspended: never squeezed on the authority's order.
    NotFrozen { instance_id: u32, pid: u32, state: AbilityState },
    /// A frozen instance: the OS loop orders the parent.
    Frozen { instance_id: u32, pid: u32 },
}

/// Whether `frame` may be taken from `sender_service_id`: `OP_MEM_SQUEEZE` only from the memory
/// authority (its kernel sender id, never a payload string); every other op passes here.
#[must_use]
pub fn mem_squeeze_frame_allowed(frame: &[u8], sender_service_id: u64, memd_sid: u64) -> bool {
    frame.get(3) != Some(&OP_MEM_SQUEEZE)
        || (sender_service_id != 0 && sender_service_id == memd_sid)
}

/// Judges one order against the broker's truth.
#[must_use]
pub fn handle_mem_squeeze(broker: &Broker, frame: &[u8]) -> MemSqueezeOutcome {
    // [A,M,ver,OP_MEM_SQUEEZE, pid:u32le]
    if frame.len() != 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return MemSqueezeOutcome::Malformed;
    }
    let pid = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let Some(inst) = broker.instance_by_pid(pid) else {
        return MemSqueezeOutcome::Unknown { pid };
    };
    match inst.state {
        AbilityState::Suspended => MemSqueezeOutcome::Frozen { instance_id: inst.instance_id, pid },
        state => MemSqueezeOutcome::NotFrozen { instance_id: inst.instance_id, pid, state },
    }
}

/// `[A,M,ver,OP_MEM_SQUEEZE, pid:u32le]` (memd's side, and tests).
#[must_use]
pub fn encode_mem_squeeze(pid: u32) -> [u8; 8] {
    let p = pid.to_le_bytes();
    [MAGIC0, MAGIC1, VERSION, OP_MEM_SQUEEZE, p[0], p[1], p[2], p[3]]
}

/// The reply's length.
pub const MEM_SQUEEZE_REPLY_LEN: usize = 9 + 5 * 4;

/// `[A,M,ver,OP_MEM_SQUEEZE|0x80, status, pid:u32le, pages, zero, kept, taken, stored: u32le]`.
#[must_use]
pub fn encode_mem_squeeze_reply(status: u8, pid: u32, s: Squeezed) -> [u8; MEM_SQUEEZE_REPLY_LEN] {
    let mut out = [0u8; MEM_SQUEEZE_REPLY_LEN];
    out[..5].copy_from_slice(&[MAGIC0, MAGIC1, VERSION, OP_MEM_SQUEEZE | OP_RESPONSE, status]);
    for (i, value) in [pid, s.pages, s.zero, s.kept, s.taken, s.stored].iter().enumerate() {
        out[5 + i * 4..9 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seen::Seen;

    const MEMD: u64 = 0x4D45_4D44;
    const WINDOWD: u64 = 0x5749_4E44;

    fn launched(broker: &mut Broker, app: &str, pid: u32) -> u32 {
        let id = broker.launch(app, "main").expect("launch");
        broker.bind_pid(id, pid).expect("bind");
        id
    }

    #[test]
    fn test_reject_mem_squeeze_from_anyone_but_memd() {
        let frame = encode_mem_squeeze(41);
        assert!(mem_squeeze_frame_allowed(&frame, MEMD, MEMD));
        assert!(!mem_squeeze_frame_allowed(&frame, WINDOWD, MEMD), "the compositor may not");
        assert!(!mem_squeeze_frame_allowed(&frame, 0, MEMD), "nobody may");
        assert!(mem_squeeze_frame_allowed(&crate::mem_kill::encode_mem_kill(41), WINDOWD, MEMD));
    }

    /// Starting, focused, hidden in its grace: not frozen — never squeezed on the order.
    #[test]
    fn test_reject_mem_squeeze_of_an_app_that_is_not_frozen() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 41);
        let order = encode_mem_squeeze(41);
        assert!(matches!(
            handle_mem_squeeze(&broker, &order),
            MemSqueezeOutcome::NotFrozen { instance_id, pid: 41, .. } if instance_id == id
        ));
        broker.seen(id, Seen::Focused, 1).expect("seen");
        assert_eq!(
            handle_mem_squeeze(&broker, &order),
            MemSqueezeOutcome::NotFrozen {
                instance_id: id,
                pid: 41,
                state: AbilityState::Foreground
            }
        );
        broker.seen(id, Seen::Hidden, 2).expect("seen");
        assert_eq!(
            handle_mem_squeeze(&broker, &order),
            MemSqueezeOutcome::NotFrozen {
                instance_id: id,
                pid: 41,
                state: AbilityState::Background
            }
        );
    }

    #[test]
    fn a_frozen_instance_is_squeezed_and_a_stranger_is_unknown() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 41);
        broker.seen(id, Seen::Hidden, 1).expect("seen");
        let _ = broker.due_freezes(1 + crate::seen::FREEZE_GRACE_NS);
        assert_eq!(
            handle_mem_squeeze(&broker, &encode_mem_squeeze(41)),
            MemSqueezeOutcome::Frozen { instance_id: id, pid: 41 }
        );
        assert_eq!(
            handle_mem_squeeze(&broker, &encode_mem_squeeze(99)),
            MemSqueezeOutcome::Unknown { pid: 99 }
        );
    }

    #[test]
    fn malformed_orders_and_the_reply_bytes() {
        let broker = Broker::new();
        let order = encode_mem_squeeze(0x0102_0304);
        assert_eq!(order, [b'A', b'M', 1, 8, 4, 3, 2, 1]);
        assert_eq!(handle_mem_squeeze(&broker, &order[..7]), MemSqueezeOutcome::Malformed);
        let mut foreign = order;
        foreign[1] = b'X';
        assert_eq!(handle_mem_squeeze(&broker, &foreign), MemSqueezeOutcome::Malformed);
        let s = Squeezed { pages: 400, zero: 12, kept: 3, taken: 52, stored: 812_000 };
        let reply = encode_mem_squeeze_reply(crate::protocol::STATUS_OK, 41, s);
        assert_eq!(&reply[..5], &[b'A', b'M', 1, 0x88, 0]);
        assert_eq!(&reply[5..9], &41u32.to_le_bytes());
        assert_eq!(&reply[9..13], &400u32.to_le_bytes());
        assert_eq!(&reply[25..29], &812_000u32.to_le_bytes());
    }
}
