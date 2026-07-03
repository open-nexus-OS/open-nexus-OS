// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Launch-handoff orchestrator — the pure authority sequence behind a
//! launcher click (resolve → caps → spawn → surface), host-tested (RFC-0065).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 tests
//!
//! Launch-handoff orchestrator — the pure authority sequence behind a launcher
//! click. RFC-0065 §Launch handoff contract:
//!
//! ```text
//! SystemUI: launcher click (app_id)
//!   → resolve via bundlemgrd   (AppRecord: launch_ability, image)
//!   → broker.launch            (instance in Created)
//!   → spawn via execd          (pid; only abilitymgr may spawn apps)
//!   → broker.start             (Created → Started)
//!   → bind surface via windowd (window id; focus owned by windowd)
//!   → broker.to_foreground     (Started → Foreground)
//! ```
//!
//! The collaborators are injected as traits so the sequence — and its failure
//! handling — is host-tested with mocks, while the OS path supplies real IPC
//! clients (execd spawn now; bundlemgrd resolve + windowd bind as their os-lite
//! surfaces land). Authorities stay split: this orchestrator owns *ordering*, not
//! resolution, spawning, or window state.

use alloc::string::String;
use alloc::vec::Vec;

use crate::lifecycle::Broker;

/// What the registry knows about an app to launch (projected from an `AppRecord`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedApp {
    /// Stable app/bundle id.
    pub app_id: String,
    /// Ability entrypoint to launch.
    pub launch_ability: String,
    /// execd image id for the app's payload ELF.
    pub image_id: u8,
}

/// Resolves an app id to its registry record (real impl: bundlemgrd client).
pub trait AppResolver {
    /// Returns the resolved app, or `None` if not installed.
    fn resolve(&self, app_id: &str) -> Option<ResolvedApp>;
}

/// Answers whether a user session is active (real impl: sessiond client,
/// TASK-0065B). App launches are session-scoped: before login (greeter) the
/// lifecycle broker refuses to launch anything — the pre-session gate at the
/// AUTHORITY, not just in the UI.
pub trait SessionGate {
    /// `true` when a user session is active (launches allowed).
    fn session_active(&self) -> bool;
}

/// Spawns an app process (real impl: execd client). Only abilitymgr calls this.
pub trait Spawner {
    /// Spawns `image_id` on behalf of `requester`; returns the new pid.
    fn spawn(&mut self, image_id: u8, requester: &str) -> Result<u32, HandoffError>;
}

/// Binds an app's surface into a window + reports the window id (real impl:
/// windowd client). Focus/window state stays owned by windowd.
pub trait SurfaceBinder {
    /// Binds the instance's surface; returns the windowd window id.
    fn bind(&mut self, instance_id: u32, app_id: &str) -> Result<u32, HandoffError>;
}

/// Failure points in the launch handoff (each maps to a distinct marker/status).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffError {
    /// No user session is active (greeter/locked) — launch refused before
    /// anything is resolved or spawned (TASK-0065B session gate).
    SessionNotReady,
    /// The app id is not installed in the registry.
    NotInstalled,
    /// The lifecycle broker rejected the launch (e.g. instance table full).
    LaunchRejected,
    /// execd failed to spawn the process.
    SpawnFailed,
    /// windowd failed to bind the surface.
    BindFailed,
    /// A lifecycle transition was rejected (should not happen on the happy path).
    LifecycleRejected,
}

/// The successful result of a launch handoff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// Broker instance id.
    pub instance_id: u32,
    /// Spawned process id.
    pub pid: u32,
    /// windowd window id the app's surface is bound to.
    pub window_id: u32,
    /// Resolved app id (for markers/recents).
    pub app_id: String,
}

/// Steps completed during a handoff, in order — the OS loop turns these into the
/// `abilitymgr:` marker ladder; tests assert the authority order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffStep {
    /// Resolved `app_id` to a registry record.
    Resolved { app_id: String },
    /// Spawned the process (pid).
    Spawned { pid: u32 },
    /// Bound the surface (window id).
    Bound { window_id: u32 },
    /// Brought the instance to the foreground.
    Foregrounded { window_id: u32 },
}

/// Runs the full launch handoff. On any failure the broker is left consistent
/// (a launched-but-failed instance is stopped) and the failing step is returned.
///
/// `steps` is appended with each completed [`HandoffStep`] so the caller can emit
/// markers in authority order without re-deriving them.
pub fn launch_app(
    broker: &mut Broker,
    gate: &dyn SessionGate,
    resolver: &dyn AppResolver,
    spawner: &mut dyn Spawner,
    binder: &mut dyn SurfaceBinder,
    app_id: &str,
    steps: &mut Vec<HandoffStep>,
) -> Result<LaunchOutcome, HandoffError> {
    // 0. Session gate (TASK-0065B): no launches before a user session is
    //    active. Refused BEFORE resolve — pre-session requests leak nothing
    //    about the registry and never touch execd.
    if !gate.session_active() {
        return Err(HandoffError::SessionNotReady);
    }
    // 1. Resolve via the registry (bundlemgrd). No spawn for unknown apps.
    let resolved = resolver.resolve(app_id).ok_or(HandoffError::NotInstalled)?;
    steps.push(HandoffStep::Resolved { app_id: resolved.app_id.clone() });

    // 2. Register the instance (Created).
    let instance_id = broker
        .launch(&resolved.app_id, &resolved.launch_ability)
        .map_err(|_| HandoffError::LaunchRejected)?;

    // 3. Spawn the process (execd). Roll back the instance on failure.
    let pid = match spawner.spawn(resolved.image_id, "abilitymgr") {
        Ok(pid) => pid,
        Err(_) => {
            let _ = broker.stop(instance_id);
            return Err(HandoffError::SpawnFailed);
        }
    };
    steps.push(HandoffStep::Spawned { pid });

    // 4. Started.
    if broker.start(instance_id).is_err() {
        let _ = broker.stop(instance_id);
        return Err(HandoffError::LifecycleRejected);
    }

    // 5. Bind the surface (windowd). Roll back on failure.
    let window_id = match binder.bind(instance_id, &resolved.app_id) {
        Ok(win) => win,
        Err(_) => {
            let _ = broker.stop(instance_id);
            return Err(HandoffError::BindFailed);
        }
    };
    steps.push(HandoffStep::Bound { window_id });

    // 6. Foreground (focus owned by windowd; broker tracks the state).
    if broker.to_foreground(instance_id).is_err() {
        let _ = broker.stop(instance_id);
        return Err(HandoffError::LifecycleRejected);
    }
    steps.push(HandoffStep::Foregrounded { window_id });

    Ok(LaunchOutcome { instance_id, pid, window_id, app_id: resolved.app_id })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lifecycle::AbilityState;
    use alloc::vec;

    struct ActiveGate;
    impl SessionGate for ActiveGate {
        fn session_active(&self) -> bool {
            true
        }
    }
    struct GreeterGate;
    impl SessionGate for GreeterGate {
        fn session_active(&self) -> bool {
            false
        }
    }

    struct CatalogResolver(Vec<ResolvedApp>);
    impl AppResolver for CatalogResolver {
        fn resolve(&self, app_id: &str) -> Option<ResolvedApp> {
            self.0.iter().find(|a| a.app_id == app_id).cloned()
        }
    }

    struct OkSpawner {
        next_pid: u32,
    }
    impl Spawner for OkSpawner {
        fn spawn(&mut self, _image_id: u8, _requester: &str) -> Result<u32, HandoffError> {
            let pid = self.next_pid;
            self.next_pid += 1;
            Ok(pid)
        }
    }
    struct FailSpawner;
    impl Spawner for FailSpawner {
        fn spawn(&mut self, _image_id: u8, _requester: &str) -> Result<u32, HandoffError> {
            Err(HandoffError::SpawnFailed)
        }
    }

    struct OkBinder {
        next_win: u32,
    }
    impl SurfaceBinder for OkBinder {
        fn bind(&mut self, _instance_id: u32, _app_id: &str) -> Result<u32, HandoffError> {
            let win = self.next_win;
            self.next_win += 1;
            Ok(win)
        }
    }
    struct FailBinder;
    impl SurfaceBinder for FailBinder {
        fn bind(&mut self, _instance_id: u32, _app_id: &str) -> Result<u32, HandoffError> {
            Err(HandoffError::BindFailed)
        }
    }

    fn catalog() -> CatalogResolver {
        CatalogResolver(vec![
            ResolvedApp {
                app_id: "search".into(),
                launch_ability: "search.MainAbility".into(),
                image_id: 10,
            },
            ResolvedApp {
                app_id: "chat".into(),
                launch_ability: "chat.MainAbility".into(),
                image_id: 11,
            },
        ])
    }

    #[test]
    fn happy_path_drives_authority_order() {
        let mut broker = Broker::new();
        let mut spawner = OkSpawner { next_pid: 1000 };
        let mut binder = OkBinder { next_win: 1 };
        let mut steps = Vec::new();

        let outcome =
            launch_app(&mut broker, &ActiveGate, &catalog(), &mut spawner, &mut binder, "search", &mut steps)
                .expect("launch ok");

        assert_eq!(outcome.app_id, "search");
        assert_eq!(outcome.pid, 1000);
        assert_eq!(outcome.window_id, 1);
        // Instance ends Foreground.
        assert_eq!(broker.state(outcome.instance_id), Some(AbilityState::Foreground));
        // Steps occurred in authority order.
        assert_eq!(
            steps,
            vec![
                HandoffStep::Resolved { app_id: "search".into() },
                HandoffStep::Spawned { pid: 1000 },
                HandoffStep::Bound { window_id: 1 },
                HandoffStep::Foregrounded { window_id: 1 },
            ]
        );
    }

    #[test]
    fn unknown_app_does_not_spawn() {
        let mut broker = Broker::new();
        let mut spawner = OkSpawner { next_pid: 1 };
        let mut binder = OkBinder { next_win: 1 };
        let mut steps = Vec::new();
        let err = launch_app(&mut broker, &ActiveGate, &catalog(), &mut spawner, &mut binder, "ghost", &mut steps)
            .unwrap_err();
        assert_eq!(err, HandoffError::NotInstalled);
        assert!(broker.is_empty(), "no instance registered for unknown app");
        assert!(steps.is_empty());
    }

    #[test]
    fn spawn_failure_rolls_back_instance() {
        let mut broker = Broker::new();
        let mut spawner = FailSpawner;
        let mut binder = OkBinder { next_win: 1 };
        let mut steps = Vec::new();
        let err = launch_app(&mut broker, &ActiveGate, &catalog(), &mut spawner, &mut binder, "chat", &mut steps)
            .unwrap_err();
        assert_eq!(err, HandoffError::SpawnFailed);
        // Instance was registered then stopped (terminal), never foregrounded.
        assert_eq!(steps, vec![HandoffStep::Resolved { app_id: "chat".into() }]);
        let inst = broker.instance(1).expect("instance exists but stopped");
        assert_eq!(inst.state, AbilityState::Stopped);
    }

    #[test]
    fn bind_failure_rolls_back_instance() {
        let mut broker = Broker::new();
        let mut spawner = OkSpawner { next_pid: 1 };
        let mut binder = FailBinder;
        let mut steps = Vec::new();
        let err = launch_app(&mut broker, &ActiveGate, &catalog(), &mut spawner, &mut binder, "chat", &mut steps)
            .unwrap_err();
        assert_eq!(err, HandoffError::BindFailed);
        assert_eq!(broker.state(1), Some(AbilityState::Stopped));
        // Resolved + Spawned happened; Bound/Foregrounded did not.
        assert_eq!(
            steps,
            vec![
                HandoffStep::Resolved { app_id: "chat".into() },
                HandoffStep::Spawned { pid: 1 },
            ]
        );
    }

    #[test]
    fn pre_session_launch_rejected_before_resolve() {
        let mut broker = Broker::new();
        let mut spawner = OkSpawner { next_pid: 1 };
        let mut binder = OkBinder { next_win: 1 };
        let mut steps = Vec::new();
        let err = launch_app(
            &mut broker,
            &GreeterGate,
            &catalog(),
            &mut spawner,
            &mut binder,
            "search",
            &mut steps,
        )
        .unwrap_err();
        assert_eq!(err, HandoffError::SessionNotReady);
        // Nothing resolved, registered, or spawned.
        assert!(broker.is_empty());
        assert!(steps.is_empty());
    }

    #[test]
    fn active_session_allows_launch() {
        let mut broker = Broker::new();
        let mut spawner = OkSpawner { next_pid: 7 };
        let mut binder = OkBinder { next_win: 3 };
        let mut steps = Vec::new();
        let outcome = launch_app(
            &mut broker,
            &ActiveGate,
            &catalog(),
            &mut spawner,
            &mut binder,
            "chat",
            &mut steps,
        )
        .expect("launch ok with active session");
        assert_eq!(outcome.app_id, "chat");
    }
}
