// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pure frame dispatch for abilitymgr — decodes a request, drives the [`Broker`],
//! and produces a response plus an optional [`Event`] the service loop turns into
//! a UART marker. Shared by the OS-lite loop and host tests (no IPC here).

use alloc::string::String;
use alloc::vec::Vec;

use crate::lifecycle::{AbilityState, Broker, LifecycleError};
use crate::protocol::*;

/// A lifecycle event worth a deterministic marker. The OS loop maps these to
/// `abilitymgr: …` lines; host tests assert on them directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A new instance was launched.
    Launched { app_id: String, instance_id: u32 },
    /// An instance changed lifecycle state.
    Transitioned { instance_id: u32, to: AbilityState },
}

/// The result of dispatching one request frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatched {
    /// The encoded response frame to send back to the caller.
    pub response: Vec<u8>,
    /// The lifecycle event that occurred, if any (drives markers).
    pub event: Option<Event>,
}

/// Decodes and applies a single request frame against `broker`.
///
/// Never panics: malformed frames produce a `STATUS_MALFORMED` response.
pub fn dispatch(broker: &mut Broker, frame: &[u8]) -> Dispatched {
    if frame.len() < MIN_FRAME_LEN || frame[0] != MAGIC0 || frame[1] != MAGIC1 {
        return Dispatched { response: resp_status(0, STATUS_MALFORMED), event: None };
    }
    if frame[2] != VERSION {
        return Dispatched { response: resp_status(frame[3], STATUS_MALFORMED), event: None };
    }
    match frame[3] {
        OP_LAUNCH => dispatch_launch(broker, frame),
        OP_TRANSITION => dispatch_transition(broker, frame),
        OP_RECENTS => dispatch_recents(broker, frame),
        other => Dispatched { response: resp_status(other, STATUS_MALFORMED), event: None },
    }
}

fn dispatch_launch(broker: &mut Broker, frame: &[u8]) -> Dispatched {
    // [A,M,ver,OP, app_len:u8, app..., abil_len:u8, abil...]
    let Some((app, rest)) = take_lp_str(&frame[4..]) else {
        return Dispatched { response: resp_status(OP_LAUNCH, STATUS_MALFORMED), event: None };
    };
    let Some((abil, _)) = take_lp_str(rest) else {
        return Dispatched { response: resp_status(OP_LAUNCH, STATUS_MALFORMED), event: None };
    };
    match broker.launch(&app, &abil) {
        Ok(id) => {
            let state = broker.state(id).unwrap_or(AbilityState::Created);
            Dispatched {
                response: resp_instance(OP_LAUNCH, STATUS_OK, id, state),
                event: Some(Event::Launched { app_id: app, instance_id: id }),
            }
        }
        Err(LifecycleError::TooManyInstances) => {
            Dispatched { response: resp_status(OP_LAUNCH, STATUS_FULL), event: None }
        }
        Err(_) => Dispatched { response: resp_status(OP_LAUNCH, STATUS_MALFORMED), event: None },
    }
}

fn dispatch_transition(broker: &mut Broker, frame: &[u8]) -> Dispatched {
    // [A,M,ver,OP, instance_id:u32le, to_state:u8]
    if frame.len() != 9 {
        return Dispatched { response: resp_status(OP_TRANSITION, STATUS_MALFORMED), event: None };
    }
    let id = u32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
    let Some(to) = AbilityState::from_wire(frame[8]) else {
        return Dispatched { response: resp_status(OP_TRANSITION, STATUS_MALFORMED), event: None };
    };
    match broker.transition(id, to) {
        Ok(state) => Dispatched {
            response: resp_instance(OP_TRANSITION, STATUS_OK, id, state),
            event: Some(Event::Transitioned { instance_id: id, to: state }),
        },
        Err(LifecycleError::UnknownInstance) => {
            Dispatched { response: resp_status(OP_TRANSITION, STATUS_UNKNOWN), event: None }
        }
        Err(LifecycleError::InvalidTransition { .. }) => Dispatched {
            response: resp_status(OP_TRANSITION, STATUS_INVALID_TRANSITION),
            event: None,
        },
        Err(LifecycleError::TooManyInstances) => {
            Dispatched { response: resp_status(OP_TRANSITION, STATUS_FULL), event: None }
        }
    }
}

fn dispatch_recents(broker: &Broker, frame: &[u8]) -> Dispatched {
    if frame.len() != 4 {
        return Dispatched { response: resp_status(OP_RECENTS, STATUS_MALFORMED), event: None };
    }
    let count = broker.recents().len() as u16;
    let mut out = header(OP_RECENTS, STATUS_OK);
    out.extend_from_slice(&count.to_le_bytes());
    Dispatched { response: out, event: None }
}

// --- encoding helpers ---

fn header(op: u8, status: u8) -> Vec<u8> {
    let mut out = Vec::with_capacity(5);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(op | OP_RESPONSE);
    out.push(status);
    out
}

fn resp_status(op: u8, status: u8) -> Vec<u8> {
    header(op, status)
}

fn resp_instance(op: u8, status: u8, instance_id: u32, state: AbilityState) -> Vec<u8> {
    let mut out = header(op, status);
    out.extend_from_slice(&instance_id.to_le_bytes());
    out.push(state.as_wire());
    out
}

/// Reads a length-prefixed (`u8` length) UTF-8 string from the front of `buf`.
/// Returns the string and the remaining bytes, or `None` if malformed.
fn take_lp_str(buf: &[u8]) -> Option<(String, &[u8])> {
    let len = *buf.first()? as usize;
    let body = buf.get(1..1 + len)?;
    let s = core::str::from_utf8(body).ok()?;
    Some((String::from(s), &buf[1 + len..]))
}

/// Encodes a launch request frame (for callers/tests).
pub fn encode_launch(app_id: &str, launch_ability: &str) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_LAUNCH);
    out.push(app_id.len() as u8);
    out.extend_from_slice(app_id.as_bytes());
    out.push(launch_ability.len() as u8);
    out.extend_from_slice(launch_ability.as_bytes());
    out
}

/// Encodes a transition request frame (for callers/tests).
pub fn encode_transition(instance_id: u32, to: AbilityState) -> Vec<u8> {
    let mut out = Vec::with_capacity(9);
    out.push(MAGIC0);
    out.push(MAGIC1);
    out.push(VERSION);
    out.push(OP_TRANSITION);
    out.extend_from_slice(&instance_id.to_le_bytes());
    out.push(to.as_wire());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp_op_status(frame: &[u8]) -> (u8, u8) {
        (frame[3], frame[4])
    }

    #[test]
    fn launch_then_transition_roundtrip() {
        let mut broker = Broker::new();
        let d = dispatch(&mut broker, &encode_launch("search", "search.main"));
        let (op, status) = resp_op_status(&d.response);
        assert_eq!(op, OP_LAUNCH | OP_RESPONSE);
        assert_eq!(status, STATUS_OK);
        let id = u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]]);
        assert_eq!(d.event, Some(Event::Launched { app_id: "search".into(), instance_id: id }));

        // Created → Started.
        let d = dispatch(&mut broker, &encode_transition(id, AbilityState::Started));
        assert_eq!(resp_op_status(&d.response), (OP_TRANSITION | OP_RESPONSE, STATUS_OK));
        assert_eq!(d.event, Some(Event::Transitioned { instance_id: id, to: AbilityState::Started }));
    }

    #[test]
    fn invalid_transition_status() {
        let mut broker = Broker::new();
        let d = dispatch(&mut broker, &encode_launch("notes", "notes.main"));
        let id = u32::from_le_bytes([d.response[5], d.response[6], d.response[7], d.response[8]]);
        // Created → Foreground is illegal (must Start first).
        let d = dispatch(&mut broker, &encode_transition(id, AbilityState::Foreground));
        assert_eq!(resp_op_status(&d.response).1, STATUS_INVALID_TRANSITION);
        assert!(d.event.is_none());
    }

    #[test]
    fn unknown_instance_status() {
        let mut broker = Broker::new();
        let d = dispatch(&mut broker, &encode_transition(42, AbilityState::Started));
        assert_eq!(resp_op_status(&d.response).1, STATUS_UNKNOWN);
    }

    #[test]
    fn malformed_frames_rejected() {
        let mut broker = Broker::new();
        assert_eq!(resp_op_status(&dispatch(&mut broker, &[]).response).1, STATUS_MALFORMED);
        assert_eq!(
            resp_op_status(&dispatch(&mut broker, &[b'X', b'Y', 1, OP_LAUNCH]).response).1,
            STATUS_MALFORMED
        );
        // Bad version.
        assert_eq!(
            resp_op_status(&dispatch(&mut broker, &[MAGIC0, MAGIC1, 99, OP_LAUNCH]).response).1,
            STATUS_MALFORMED
        );
    }

    #[test]
    fn recents_count_reported() {
        let mut broker = Broker::new();
        dispatch(&mut broker, &encode_launch("search", "s.main"));
        dispatch(&mut broker, &encode_launch("chat", "c.main"));
        let d = dispatch(&mut broker, &[MAGIC0, MAGIC1, VERSION, OP_RECENTS]);
        assert_eq!(resp_op_status(&d.response), (OP_RECENTS | OP_RESPONSE, STATUS_OK));
        let count = u16::from_le_bytes([d.response[5], d.response[6]]);
        assert_eq!(count, 2);
    }
}
