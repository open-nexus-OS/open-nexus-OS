// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Session state machine — the host-testable SSOT for sessiond's
//! authority (TASK-0065B). Pure: no IPC, no manifests, no markers.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `cargo test -p sessiond`
//! INVARIANTS:
//! - `Greeter → Active` is the only transition today; `Locked` is designed-in
//!   but reserved (lock() rejects) so the wire value and state shape are stable
//!   before the lock-screen track lands
//! - the machine never validates user ids — the registry does; it only tracks
//!   WHICH registered user (by index) owns the session

/// Session lifecycle state. The payload is the index into the user registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// No session — the greeter owns the display.
    Greeter,
    /// A user session is active.
    Active(usize),
    /// The active session is locked (reserved — no transition reaches this yet).
    Locked(usize),
}

/// Rejection reasons for invalid transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateError {
    /// The requested transition is invalid in the current state.
    WrongState,
    /// The transition exists in the model but is not implemented yet.
    Reserved,
}

impl SessionState {
    /// Wire value per `nexus_abi::sessiond` (STATE_GREETER/ACTIVE/LOCKED).
    pub fn as_wire(&self) -> u8 {
        match self {
            Self::Greeter => 0,
            Self::Active(_) => 1,
            Self::Locked(_) => 2,
        }
    }

    /// The active user's registry index, when a session exists.
    pub fn active_user(&self) -> Option<usize> {
        match self {
            Self::Greeter => None,
            Self::Active(idx) | Self::Locked(idx) => Some(*idx),
        }
    }

    /// Greeter → Active: the login transition. Auth docks in front of this
    /// call later; the machine itself stays auth-agnostic.
    pub fn login(&mut self, user_idx: usize) -> Result<(), StateError> {
        match self {
            Self::Greeter => {
                *self = Self::Active(user_idx);
                Ok(())
            }
            _ => Err(StateError::WrongState),
        }
    }

    /// Active → Locked: reserved until the lock-screen track lands.
    pub fn lock(&mut self) -> Result<(), StateError> {
        match self {
            Self::Active(_) => Err(StateError::Reserved),
            _ => Err(StateError::WrongState),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeter_login_activates() {
        let mut state = SessionState::Greeter;
        assert_eq!(state.active_user(), None);
        assert_eq!(state.as_wire(), 0);
        state.login(0).expect("login from greeter");
        assert_eq!(state, SessionState::Active(0));
        assert_eq!(state.active_user(), Some(0));
        assert_eq!(state.as_wire(), 1);
    }

    #[test]
    fn login_wrong_state_rejected() {
        let mut state = SessionState::Active(0);
        assert_eq!(state.login(1), Err(StateError::WrongState));
        assert_eq!(state, SessionState::Active(0));
    }

    #[test]
    fn lock_reserved() {
        let mut state = SessionState::Active(0);
        assert_eq!(state.lock(), Err(StateError::Reserved));
        let mut greeter = SessionState::Greeter;
        assert_eq!(greeter.lock(), Err(StateError::WrongState));
    }
}
