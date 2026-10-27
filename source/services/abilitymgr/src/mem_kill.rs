// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The memory authority's kill order (TASK-0333 6e-2, RFC-0100 §3, ADR-0086). memd
//! decides WHO goes; this service holds the lifecycle truth and the parent's ear: it refuses
//! an order against the foreground (or an instance still starting), finds the instance by
//! the process the order names, and lets the OS loop carry `OP_KILL` to execd (ADR-0082). The
//! frame is taken from memd's kernel sender id alone — a forged order would end an app the
//! user is looking at. Pure; the OS loop owns the route.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal (string-identical to memd's `order.rs`)
//! TEST_COVERAGE: tests below (`test_reject_mem_kill_from_anyone_but_memd`,
//!   `test_reject_mem_kill_of_foreground`, the victim, malformed)

use crate::lifecycle::{AbilityState, Broker};
use crate::protocol::{MAGIC0, MAGIC1, OP_MEM_KILL, OP_RESPONSE, VERSION};

/// What one `OP_MEM_KILL` frame came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemKillOutcome {
    /// Not the 8-byte frame.
    Malformed,
    /// No live instance runs as that process.
    Unknown { pid: u32 },
    /// The instance is the foreground (or still starting): never on the authority's order.
    Foreground { instance_id: u32, pid: u32 },
    /// A legitimate victim: the OS loop orders the parent.
    Victim { instance_id: u32, pid: u32, state: AbilityState },
}

/// Whether `frame` may be taken from `sender_service_id`: `OP_MEM_KILL` only from the memory
/// authority (its kernel sender id, never a payload string); every other op passes here.
#[must_use]
pub fn mem_kill_frame_allowed(frame: &[u8], sender_service_id: u64, memd_sid: u64) -> bool {
    frame.get(3) != Some(&OP_MEM_KILL) || (sender_service_id != 0 && sender_service_id == memd_sid)
}

/// Judges one order against the broker's truth.
#[must_use]
pub fn handle_mem_kill(broker: &Broker, frame: &[u8]) -> MemKillOutcome {
    // [A,M,ver,OP_MEM_KILL, pid:u32le]
    if frame.len() != 8 || frame[0] != MAGIC0 || frame[1] != MAGIC1 || frame[2] != VERSION {
        return MemKillOutcome::Malformed;
    }
    let pid = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let Some(inst) = broker.instance_by_pid(pid) else {
        return MemKillOutcome::Unknown { pid };
    };
    match inst.state {
        AbilityState::Created | AbilityState::Started | AbilityState::Foreground => {
            MemKillOutcome::Foreground { instance_id: inst.instance_id, pid }
        }
        state => MemKillOutcome::Victim { instance_id: inst.instance_id, pid, state },
    }
}

/// `[A,M,ver,OP_MEM_KILL, pid:u32le]` (memd's side, and tests).
#[must_use]
pub fn encode_mem_kill(pid: u32) -> [u8; 8] {
    let p = pid.to_le_bytes();
    [MAGIC0, MAGIC1, VERSION, OP_MEM_KILL, p[0], p[1], p[2], p[3]]
}

/// `[A,M,ver,OP_MEM_KILL|0x80, status, pid:u32le]`.
#[must_use]
pub fn encode_mem_kill_reply(status: u8, pid: u32) -> [u8; 9] {
    let p = pid.to_le_bytes();
    [MAGIC0, MAGIC1, VERSION, OP_MEM_KILL | OP_RESPONSE, status, p[0], p[1], p[2], p[3]]
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
    fn test_reject_mem_kill_from_anyone_but_memd() {
        let frame = encode_mem_kill(41);
        assert!(mem_kill_frame_allowed(&frame, MEMD, MEMD));
        assert!(!mem_kill_frame_allowed(&frame, WINDOWD, MEMD), "the compositor may not");
        assert!(!mem_kill_frame_allowed(&frame, 0, MEMD), "nobody may");
        // Any other op passes this gate (its own rules apply).
        assert!(mem_kill_frame_allowed(
            &crate::wire::encode_launch("notes", "main"),
            WINDOWD,
            MEMD
        ));
    }

    #[test]
    fn test_reject_mem_kill_of_foreground() {
        let mut broker = Broker::new();
        let starting = launched(&mut broker, "notes", 41);
        assert_eq!(
            handle_mem_kill(&broker, &encode_mem_kill(41)),
            MemKillOutcome::Foreground { instance_id: starting, pid: 41 },
            "an app still starting is never the authority's victim"
        );
        broker.seen(starting, Seen::Focused, 1).expect("seen");
        assert_eq!(
            handle_mem_kill(&broker, &encode_mem_kill(41)),
            MemKillOutcome::Foreground { instance_id: starting, pid: 41 }
        );
    }

    #[test]
    fn a_hidden_or_frozen_instance_is_the_victim_and_a_stranger_is_unknown() {
        let mut broker = Broker::new();
        let id = launched(&mut broker, "notes", 41);
        broker.seen(id, Seen::Hidden, 1).expect("seen");
        assert_eq!(
            handle_mem_kill(&broker, &encode_mem_kill(41)),
            MemKillOutcome::Victim { instance_id: id, pid: 41, state: AbilityState::Background }
        );
        let _ = broker.due_freezes(1 + crate::seen::FREEZE_GRACE_NS);
        assert_eq!(
            handle_mem_kill(&broker, &encode_mem_kill(41)),
            MemKillOutcome::Victim { instance_id: id, pid: 41, state: AbilityState::Suspended }
        );
        assert_eq!(
            handle_mem_kill(&broker, &encode_mem_kill(99)),
            MemKillOutcome::Unknown { pid: 99 }
        );
    }

    #[test]
    fn malformed_orders_and_the_reply_bytes() {
        let broker = Broker::new();
        assert_eq!(handle_mem_kill(&broker, &encode_mem_kill(1)[..7]), MemKillOutcome::Malformed);
        let mut foreign = encode_mem_kill(1);
        foreign[2] = 9;
        assert_eq!(handle_mem_kill(&broker, &foreign), MemKillOutcome::Malformed);
        assert_eq!(encode_mem_kill(0x0102_0304), [b'A', b'M', 1, 7, 4, 3, 2, 1]);
        assert_eq!(
            encode_mem_kill_reply(crate::protocol::STATUS_OK, 0x0102_0304),
            [b'A', b'M', 1, 0x87, 0, 4, 3, 2, 1]
        );
    }
}
