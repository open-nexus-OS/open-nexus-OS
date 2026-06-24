// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Pure ability-lifecycle state machine + recents + launch caps gate —
//! the broker core, host-tested SSOT for transition ordering (RFC-0065).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 9 tests
//!
//! Pure ability-lifecycle state machine + recents — the broker core.
//!
//! Host-testable and `no_std` (alloc only). This is the SSOT for lifecycle
//! ordering: the OS-lite service loop and the host CLI both drive this `Broker`,
//! so the transition rules are proven once and reused. RFC-0065 §Lifecycle.

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// Lifecycle state of a running ability instance (a managed UI scene).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityState {
    /// Allocated, not yet started.
    Created,
    /// Started, not yet placed foreground/background.
    Started,
    /// Visible + focused.
    Foreground,
    /// Live but not focused.
    Background,
    /// Backgrounded and suspended (no CPU).
    Suspended,
    /// Terminated.
    Stopped,
}

impl AbilityState {
    /// Stable wire encoding for the IPC protocol.
    pub fn as_wire(self) -> u8 {
        match self {
            AbilityState::Created => 0,
            AbilityState::Started => 1,
            AbilityState::Foreground => 2,
            AbilityState::Background => 3,
            AbilityState::Suspended => 4,
            AbilityState::Stopped => 5,
        }
    }

    /// Decodes a wire state code, or `None` if out of range.
    pub fn from_wire(code: u8) -> Option<Self> {
        Some(match code {
            0 => AbilityState::Created,
            1 => AbilityState::Started,
            2 => AbilityState::Foreground,
            3 => AbilityState::Background,
            4 => AbilityState::Suspended,
            5 => AbilityState::Stopped,
            _ => return None,
        })
    }
}

/// Errors from an invalid lifecycle operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleError {
    /// No instance with the given id.
    UnknownInstance,
    /// The requested transition is not allowed from the current state.
    InvalidTransition { from: AbilityState, to: AbilityState },
    /// The instance table is at `MAX_INSTANCES`.
    TooManyInstances,
    /// The app's manifest declares a capability the platform does not recognize;
    /// the launch is denied (fail-closed). RFC-0065 launch authority.
    UnsupportedCapability,
}

/// A launched ability instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbilityInstance {
    /// Broker-assigned instance id (stable for the instance's lifetime).
    pub instance_id: u32,
    /// The app/bundle id this instance belongs to (e.g. `"search"`).
    pub app_id: String,
    /// The ability entrypoint that was launched.
    pub launch_ability: String,
    /// Current lifecycle state.
    pub state: AbilityState,
    /// The manifest-declared, validated capabilities granted to this instance —
    /// the exact set the per-app spawn will request (RFC-0065 launch authority).
    pub granted_caps: Vec<String>,
}

/// Maximum concurrently-tracked instances (bounded resource).
pub const MAX_INSTANCES: usize = 32;
/// Maximum recents entries retained (MRU window).
pub const MAX_RECENTS: usize = 16;

/// `true` if `from → to` is a legal lifecycle transition.
pub fn valid_transition(from: AbilityState, to: AbilityState) -> bool {
    use AbilityState::*;
    match (from, to) {
        (Created, Started) => true,
        (Started, Foreground) | (Started, Background) => true,
        (Foreground, Background) | (Background, Foreground) => true,
        (Background, Suspended) => true,
        (Suspended, Background) | (Suspended, Foreground) => true,
        // Stop is reachable from any live state, but Stopped is terminal.
        (Stopped, _) => false,
        (_, Stopped) => true,
        _ => false,
    }
}

/// The lifecycle broker: owns running instances + a recents (MRU) list.
///
/// The broker is authority for *what is running and in what state*; it does not
/// spawn processes itself (that is execd, called by the OS service loop in P3).
#[derive(Debug, Default)]
pub struct Broker {
    instances: BTreeMap<u32, AbilityInstance>,
    next_id: u32,
    /// Instance ids in most-recently-used order (front = most recent).
    recents: Vec<u32>,
}

impl Broker {
    /// Creates an empty broker (ids start at 1).
    pub fn new() -> Self {
        Self { instances: BTreeMap::new(), next_id: 1, recents: Vec::new() }
    }

    /// Launches a new instance in `Created` with no declared capabilities.
    /// Convenience for callers that do not gate on caps (e.g. host CLI demos);
    /// the live OS path uses [`launch_with_caps`](Self::launch_with_caps).
    pub fn launch(&mut self, app_id: &str, launch_ability: &str) -> Result<u32, LifecycleError> {
        self.launch_with_caps(app_id, launch_ability, &[])
    }

    /// Launches a new instance in `Created`, enforcing the app's manifest-declared
    /// capabilities: every `required_cap` must be a known platform permission or
    /// the launch is DENIED (`UnsupportedCapability`, fail-closed). This is the
    /// RFC-0065 launch authority — an ability never runs while requesting a
    /// permission the system does not recognize. The validated set is recorded on
    /// the instance as `granted_caps` (the spawn requests exactly these).
    ///
    /// The caller drives `Started` next (and, in the OS path, the execd spawn +
    /// windowd surface bind around it).
    pub fn launch_with_caps(
        &mut self,
        app_id: &str,
        launch_ability: &str,
        required_caps: &[&str],
    ) -> Result<u32, LifecycleError> {
        if self.instances.len() >= MAX_INSTANCES {
            return Err(LifecycleError::TooManyInstances);
        }
        // Fail-closed permission check against the known set.
        if crate::caps::first_unknown(required_caps).is_some() {
            return Err(LifecycleError::UnsupportedCapability);
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.instances.insert(
            id,
            AbilityInstance {
                instance_id: id,
                app_id: String::from(app_id),
                launch_ability: String::from(launch_ability),
                state: AbilityState::Created,
                granted_caps: required_caps.iter().map(|c| String::from(*c)).collect(),
            },
        );
        self.touch_recent(id);
        Ok(id)
    }

    /// Applies a validated transition; returns the new state.
    pub fn transition(
        &mut self,
        id: u32,
        to: AbilityState,
    ) -> Result<AbilityState, LifecycleError> {
        let inst = self.instances.get_mut(&id).ok_or(LifecycleError::UnknownInstance)?;
        if !valid_transition(inst.state, to) {
            return Err(LifecycleError::InvalidTransition { from: inst.state, to });
        }
        inst.state = to;
        // Foregrounding bumps the instance to the front of recents.
        if to == AbilityState::Foreground {
            self.touch_recent(id);
        }
        Ok(to)
    }

    /// Convenience: `Created → Started`.
    pub fn start(&mut self, id: u32) -> Result<AbilityState, LifecycleError> {
        self.transition(id, AbilityState::Started)
    }

    /// Convenience: bring an instance to the foreground.
    pub fn to_foreground(&mut self, id: u32) -> Result<AbilityState, LifecycleError> {
        self.transition(id, AbilityState::Foreground)
    }

    /// Convenience: send an instance to the background.
    pub fn to_background(&mut self, id: u32) -> Result<AbilityState, LifecycleError> {
        self.transition(id, AbilityState::Background)
    }

    /// Convenience: suspend a backgrounded instance.
    pub fn suspend(&mut self, id: u32) -> Result<AbilityState, LifecycleError> {
        self.transition(id, AbilityState::Suspended)
    }

    /// Convenience: resume a suspended instance to the background.
    pub fn resume(&mut self, id: u32) -> Result<AbilityState, LifecycleError> {
        self.transition(id, AbilityState::Background)
    }

    /// Convenience: stop an instance (terminal).
    pub fn stop(&mut self, id: u32) -> Result<AbilityState, LifecycleError> {
        self.transition(id, AbilityState::Stopped)
    }

    /// Current state of an instance, if it exists.
    pub fn state(&self, id: u32) -> Option<AbilityState> {
        self.instances.get(&id).map(|i| i.state)
    }

    /// Borrows an instance, if it exists.
    pub fn instance(&self, id: u32) -> Option<&AbilityInstance> {
        self.instances.get(&id)
    }

    /// Number of tracked instances (including `Stopped`, until reaped).
    pub fn len(&self) -> usize {
        self.instances.len()
    }

    /// `true` if no instances are tracked.
    pub fn is_empty(&self) -> bool {
        self.instances.is_empty()
    }

    /// Recents in MRU order (front = most recent), resolved to instances.
    /// Stopped instances remain listed (recents includes recently-closed apps).
    pub fn recents(&self) -> Vec<&AbilityInstance> {
        self.recents.iter().filter_map(|id| self.instances.get(id)).collect()
    }

    /// Moves `id` to the front of recents, capping the list at `MAX_RECENTS`.
    fn touch_recent(&mut self, id: u32) {
        if let Some(pos) = self.recents.iter().position(|&r| r == id) {
            self.recents.remove(pos);
        }
        self.recents.insert(0, id);
        if self.recents.len() > MAX_RECENTS {
            self.recents.truncate(MAX_RECENTS);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_starts_in_created() {
        let mut broker = Broker::new();
        let id = broker.launch("search", "search.main").expect("launch");
        assert_eq!(broker.state(id), Some(AbilityState::Created));
        let inst = broker.instance(id).expect("instance");
        assert_eq!(inst.app_id, "search");
        assert_eq!(inst.launch_ability, "search.main");
    }

    #[test]
    fn full_lifecycle_ordering() {
        let mut broker = Broker::new();
        let id = broker.launch("notes", "notes.main").expect("launch");
        assert_eq!(broker.start(id).unwrap(), AbilityState::Started);
        assert_eq!(broker.to_foreground(id).unwrap(), AbilityState::Foreground);
        assert_eq!(broker.to_background(id).unwrap(), AbilityState::Background);
        assert_eq!(broker.suspend(id).unwrap(), AbilityState::Suspended);
        assert_eq!(broker.resume(id).unwrap(), AbilityState::Background);
        assert_eq!(broker.to_foreground(id).unwrap(), AbilityState::Foreground);
        assert_eq!(broker.stop(id).unwrap(), AbilityState::Stopped);
    }

    #[test]
    fn fg_bg_roundtrip() {
        let mut broker = Broker::new();
        let id = broker.launch("chat", "chat.main").expect("launch");
        broker.start(id).unwrap();
        broker.to_foreground(id).unwrap();
        broker.to_background(id).unwrap();
        broker.to_foreground(id).unwrap();
        assert_eq!(broker.state(id), Some(AbilityState::Foreground));
    }

    #[test]
    fn rejects_out_of_order_transition() {
        let mut broker = Broker::new();
        let id = broker.launch("search", "search.main").expect("launch");
        // Cannot go Created → Foreground without Started.
        let err = broker.to_foreground(id).unwrap_err();
        assert_eq!(
            err,
            LifecycleError::InvalidTransition {
                from: AbilityState::Created,
                to: AbilityState::Foreground
            }
        );
    }

    #[test]
    fn stopped_is_terminal() {
        let mut broker = Broker::new();
        let id = broker.launch("notes", "notes.main").expect("launch");
        broker.start(id).unwrap();
        broker.stop(id).unwrap();
        assert!(matches!(
            broker.start(id).unwrap_err(),
            LifecycleError::InvalidTransition { from: AbilityState::Stopped, .. }
        ));
    }

    #[test]
    fn unknown_instance_errors() {
        let mut broker = Broker::new();
        assert_eq!(broker.to_foreground(999).unwrap_err(), LifecycleError::UnknownInstance);
    }

    #[test]
    fn recents_is_mru_ordered() {
        let mut broker = Broker::new();
        let a = broker.launch("search", "search.main").unwrap();
        let b = broker.launch("chat", "chat.main").unwrap();
        // b is most-recent after launch.
        let ids: Vec<u32> = broker.recents().iter().map(|i| i.instance_id).collect();
        assert_eq!(ids, vec![b, a]);
        // Foregrounding a bumps it to front.
        broker.start(a).unwrap();
        broker.to_foreground(a).unwrap();
        let ids: Vec<u32> = broker.recents().iter().map(|i| i.instance_id).collect();
        assert_eq!(ids, vec![a, b]);
    }

    #[test]
    fn recents_bounded() {
        let mut broker = Broker::new();
        for i in 0..(MAX_RECENTS + 4) {
            let mut name = String::from("app");
            name.push_str(&i.to_string());
            broker.launch(&name, "main").unwrap();
        }
        assert_eq!(broker.recents().len(), MAX_RECENTS);
    }

    #[test]
    fn wire_state_roundtrip() {
        for s in [
            AbilityState::Created,
            AbilityState::Started,
            AbilityState::Foreground,
            AbilityState::Background,
            AbilityState::Suspended,
            AbilityState::Stopped,
        ] {
            assert_eq!(AbilityState::from_wire(s.as_wire()), Some(s));
        }
        assert_eq!(AbilityState::from_wire(99), None);
    }
}
