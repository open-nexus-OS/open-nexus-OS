// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The compositor is back (RFC-0112, amendment 2026-11-02; TASK-0348 Phase 6). execd is
//! the parent of every app process and the owner of every app's event channel (init mints the
//! pair for it): it keeps the SEND half of each channel — the owner is no peer of its own
//! channel, so the window-close EOF still fires — and when init says a respawned windowd is
//! wired (`nexus_wire::execd::OP_COMPOSITOR_BACK`, init's kernel id alone, one-way: execd's
//! answers ride a response endpoint its spawn callers read), it re-pins every live app's windowd
//! slots from its own (init re-pinned those) over the dead capabilities (REPLACE_DEAD) and sends
//! each app `OP_COMPOSITOR_BACK` on its channel, a fresh SEND of the channel moved along — the
//! app gave its last one to the dead compositor; with this one it attaches again and draws. A
//! notice whose channel is full (a frozen app's, before the lifecycle authority thawed it) is
//! OWED: the channel is armed writable on execd's waitset and the notice goes once it has room —
//! never a wait on an app, never a lost notice. Then the new compositor is told WHOM execd told
//! (`OP_APPS_ALIVE`, Phase 3): the ghosts of every other owner are of apps that are gone.
//! OWNERS: @runtime @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the restart lane (`execd: compositor back (apps=`)

use super::reaper::Reaper;
use super::{State, TrackedChild, IMG_APPHOST};
use nexus_abi::Rights;
use nexus_display_proto::surface_restart::APPS_ALIVE_MAX;
use nexus_display_proto::surface_restart::{encode_apps_alive, encode_compositor_back};
use nexus_service_topology::slots::{app_child, execd as topo};

impl TrackedChild {
    pub(super) fn new(pid: u32, image_id: u8, sid: u64) -> Self {
        Self {
            pid,
            image_id,
            sid,
            exit: None,
            reported: false,
            events_send: None,
            back_owed: false,
        }
    }
}

/// The kernel service id a child spawned under `name` carries (0: nameless).
pub(super) fn sid_of(name: Option<&crate::identity::AppServiceName>) -> u64 {
    name.map_or(0, |n| nexus_abi::service_id_from_name(n.as_str().as_bytes()))
}

impl TrackedChild {
    /// The child is gone: the SEND of its channel execd kept goes with it.
    pub(super) fn forget_events(&mut self) {
        if let Some(slot) = self.events_send.take() {
            let _ = nexus_abi::cap_close(slot);
        }
    }
}

impl State {
    /// Keeps the SEND half of `pid`'s event channel (`None`: the grant kept nothing).
    pub(super) fn keep_events(&mut self, pid: u32, send: Option<u32>) {
        match (self.children.iter_mut().find(|c| c.pid == pid), send) {
            (Some(child), send) => child.events_send = send,
            (None, Some(slot)) => {
                let _ = nexus_abi::cap_close(slot);
            }
            (None, None) => {}
        }
    }
}

/// Whether `frame` is init's notice — handled before the request path, never answered.
pub(super) fn is_notice(frame: &[u8]) -> bool {
    nexus_wire::execd::is_compositor_back(frame)
}

/// init's notice: every live app gets its windowd slots back and is told — or owes it.
pub(super) fn handle(state: &mut State, sender_service_id: u64, reaper: Option<&Reaper>) {
    if sender_service_id != nexus_abi::service_id_from_name(b"init-lite") {
        let _ = nexus_abi::debug_println("execd: compositor back denied (not init)");
        return;
    }
    let (mut told, mut owed, mut failed) = (0u32, 0u32, 0u32);
    for child in state.children.iter_mut().filter(|c| c.image_id == IMG_APPHOST && c.exit.is_none())
    {
        let route = repin(child.pid, topo::WINDOWD.send, Rights::SEND, app_child::WINDOWD.send)
            && repin(child.pid, topo::WINDOWD.recv, Rights::RECV, app_child::WINDOWD.recv);
        match child.events_send.filter(|_| route).map(tell) {
            Some(Told::Yes) => told += 1,
            Some(Told::Full) if child.owe(reaper) => owed += 1,
            _ => failed += 1,
        }
    }
    nexus_abi::DebugLine::default()
        .put(b"execd: compositor back (apps=")
        .u64(u64::from(told))
        .put(b" owed=")
        .u64(u64::from(owed))
        .put(b" failed=")
        .u64(u64::from(failed))
        .put(b")")
        .emit();
    state.apps_alive_owed = !tell_alive(state) && arm_alive(reaper);
}

/// windowd's queue had no room for the list: execd wakes when it has (`retry_owed`).
fn arm_alive(reaper: Option<&Reaper>) -> bool {
    let armed = reaper.is_some_and(|r| r.arm_writable(topo::WINDOWD.send));
    if !armed {
        let _ = nexus_abi::debug_println("execd: FAIL apps alive owed (no writable arm)");
    }
    armed
}

/// `OP_APPS_ALIVE` to the new compositor: the kernel ids of every live app (the owners whose
/// ghosts may wait for a rejoin). `false`: windowd's queue had no room — owed (`retry_owed`).
/// More live apps than one frame carries: the first `APPS_ALIVE_MAX` are named (the rest's
/// ghosts close; their rejoin opens a fresh window) — said.
fn tell_alive(state: &State) -> bool {
    let mut owners = [0u64; APPS_ALIVE_MAX];
    let mut n = 0usize;
    let live = state.children.iter().filter(|c| c.image_id == IMG_APPHOST && c.exit.is_none());
    for child in live.filter(|c| c.sid != 0) {
        if n == APPS_ALIVE_MAX {
            let _ = nexus_abi::debug_println("execd: apps alive truncated (more than the frame)");
            break;
        }
        if !owners[..n].contains(&child.sid) {
            owners[n] = child.sid;
            n += 1;
        }
    }
    let mut frame = [0u8; 512];
    let Some(len) = encode_apps_alive(&owners[..n], &mut frame) else { return true };
    match nexus_ipc::exchange::send_nonblocking(topo::WINDOWD.send, &frame[..len]) {
        Ok(()) => {
            nexus_abi::DebugLine::default()
                .put(b"execd: apps alive told (n=")
                .u64(n as u64)
                .put(b")")
                .emit();
            true
        }
        Err(nexus_ipc::IpcError::Kernel(nexus_abi::IpcError::QueueFull)) => false,
        Err(nexus_ipc::IpcError::NoSpace) => false,
        Err(_) => {
            let _ = nexus_abi::debug_println("execd: FAIL apps alive (compositor unreachable)");
            true
        }
    }
}

/// A wake of execd's waitset: every owed notice is tried again — a channel armed writable has
/// room, or its app is gone. Cheap when nothing is owed (one pass over the children).
pub(super) fn retry_owed(state: &mut State, reaper: Option<&Reaper>) {
    if state.apps_alive_owed {
        state.apps_alive_owed = !tell_alive(state) && arm_alive(reaper);
    }
    for child in state.children.iter_mut().filter(|c| c.back_owed) {
        child.back_owed = false;
        let Some(send) = child.events_send.filter(|_| child.exit.is_none()) else { continue };
        match tell(send) {
            Told::Yes => {
                nexus_abi::DebugLine::default()
                    .put(b"execd: compositor back told (pid=")
                    .u64(u64::from(child.pid))
                    .put(b")")
                    .emit();
            }
            Told::Full => {
                let _ = child.owe(reaper);
            }
            Told::No => {}
        }
    }
}

impl TrackedChild {
    /// The notice could not be queued: owed until the channel has room. `false` when nothing can
    /// wake execd for it (no waitset, or no member left) — named, then counted as failed.
    fn owe(&mut self, reaper: Option<&Reaper>) -> bool {
        let armed = self.events_send.zip(reaper).is_some_and(|(send, r)| r.arm_writable(send));
        if !armed {
            let _ = nexus_abi::debug_println("execd: FAIL compositor back owed (no writable arm)");
        }
        self.back_owed = armed;
        armed
    }
}

/// A clone of execd's own windowd slot into the app's, over the dead capability there.
fn repin(pid: u32, own_slot: u32, rights: Rights, child_slot: u32) -> bool {
    let Ok(clone) = nexus_abi::cap_clone(own_slot) else { return false };
    let pinned = nexus_abi::cap_transfer_to_slot_replace_dead(pid, clone, rights, child_slot);
    let _ = nexus_abi::cap_close(clone);
    pinned.is_ok()
}

/// What a notice came to.
enum Told {
    Yes,
    /// The app's queue is full (a frozen app's refuses at once): owed.
    Full,
    /// The app or its channel is gone.
    No,
}

/// `OP_COMPOSITOR_BACK` on the app's channel, a fresh SEND of the channel moved along. Never
/// waits on an app: a full queue makes the notice owed (`retry_owed`).
fn tell(events_send: u32) -> Told {
    use nexus_abi::IpcError as K;
    let Ok(clone) = nexus_abi::cap_clone(events_send) else { return Told::No };
    let frame = encode_compositor_back();
    let sent = nexus_ipc::exchange::send_with_cap_nonblocking(events_send, &frame, clone);
    if sent.is_ok() {
        return Told::Yes;
    }
    // A refused send rolls the moved capability back into our slot.
    let _ = nexus_abi::cap_close(clone);
    match sent {
        Err(nexus_ipc::IpcError::Kernel(K::QueueFull | K::Frozen)) => Told::Full,
        _ => Told::No,
    }
}
