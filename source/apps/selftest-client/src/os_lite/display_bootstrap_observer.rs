// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Observer-only visible display bootstrap checks for the service-owned
//!   `windowd -> fbdevd -> ramfb` path.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU marker ladder plus host observer-boundary tests.
//! ADR: docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md

extern crate alloc;

use alloc::format;
use core::time::Duration;
use input_live_protocol::{decode_visible_state, encode_get_visible_state, VisibleState};
use nexus_abi::{cap_clone, debug_println, yield_};
use nexus_ipc::{Client as _, Wait};

use crate::os_lite::boot_cfg;
use crate::os_lite::display_observer::{
    display_bootstrap_ready, emit_missing_visible_input_bits, interactive_scene_ready,
    ProofVisibleInputWitness,
};
use crate::os_lite::ipc::clients::cached_reply_client;
use crate::os_lite::ipc::routing::route_with_retry;
use crate::runtime_mode::RuntimeMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapFailure {
    DisplayServiceEvidence,
    VisibleInputEvidence,
    InteractiveSceneEvidence,
}

pub(crate) struct BootstrapEvidence {
    pub(crate) runtime_mode: RuntimeMode,
    pub(crate) display: ObservedDisplayEvidence,
    pub(crate) proof: Option<ProofBootstrapEvidence>,
    pub(crate) interactive: Option<InteractiveBootstrapEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObservedDisplayEvidence {
    pub(crate) backend_visible: bool,
    pub(crate) first_scanout_ready: bool,
    pub(crate) systemui_first_frame: bool,
}

pub(crate) struct ProofBootstrapEvidence {
    pub(crate) visible_state: VisibleState,
}

pub(crate) struct InteractiveBootstrapEvidence {
    pub(crate) scene_ready: bool,
    pub(crate) full_window_visible: bool,
    pub(crate) click_target_visible: bool,
    pub(crate) keyboard_target_visible: bool,
}

pub(crate) fn enabled() -> bool {
    boot_cfg::display_bootstrap_enabled()
}

pub(crate) fn run() -> Option<BootstrapEvidence> {
    match run_result() {
        Ok(evidence) => Some(evidence),
        Err(err) => {
            let _ = debug_println(err.log_label());
            None
        }
    }
}

pub(crate) fn run_result() -> Result<BootstrapEvidence, BootstrapFailure> {
    let runtime_mode = boot_cfg::runtime_mode_with_retry().unwrap_or(RuntimeMode::Proof);
    let display = observe_display_evidence()?;
    match runtime_mode {
        RuntimeMode::Proof => {
            let visible_state = observe_live_visible_input_proof()?;
            Ok(BootstrapEvidence {
                runtime_mode,
                display,
                proof: Some(ProofBootstrapEvidence { visible_state }),
                interactive: None,
            })
        }
        RuntimeMode::InteractiveMinimal | RuntimeMode::InteractiveFull => {
            let state = observe_interactive_scene_state()?;
            Ok(BootstrapEvidence {
                runtime_mode,
                display,
                proof: None,
                interactive: Some(InteractiveBootstrapEvidence {
                    scene_ready: state.scene_ready,
                    full_window_visible: state.full_window_visible,
                    click_target_visible: state.click_target_visible,
                    keyboard_target_visible: state.keyboard_target_visible,
                }),
            })
        }
    }
}

pub(crate) fn observe_display_evidence() -> Result<ObservedDisplayEvidence, BootstrapFailure> {
    let display_state = observe_display_bootstrap_state()?;
    Ok(ObservedDisplayEvidence {
        backend_visible: display_state.backend_visible,
        first_scanout_ready: display_state.display_scanout_ready,
        systemui_first_frame: display_state.systemui_first_frame_visible,
    })
}

impl BootstrapFailure {
    #[must_use]
    fn log_label(self) -> &'static str {
        match self {
            Self::DisplayServiceEvidence => "bootstrap: failed fbdevd-evidence",
            Self::VisibleInputEvidence => "bootstrap: failed visible-input-evidence",
            Self::InteractiveSceneEvidence => "bootstrap: failed interactive-scene-evidence",
        }
    }
}

pub(crate) fn interactive_live_tick() -> Option<VisibleState> {
    fetch_live_visible_state()
}

fn fetch_live_visible_state() -> Option<VisibleState> {
    const VISIBLE_STATE_RPC_TIMEOUT_MS: u64 = 50;
    let wait = Wait::Timeout(Duration::from_millis(VISIBLE_STATE_RPC_TIMEOUT_MS));
    let client = route_with_retry("fbdevd").ok()?;
    let reply = cached_reply_client().ok()?;
    let (reply_send_slot, _) = reply.slots();
    let reply_send_clone = cap_clone(reply_send_slot).ok()?;
    let request = encode_get_visible_state();
    client.send_with_cap_move_wait(&request, reply_send_clone, wait).ok()?;
    let frame = reply.recv(wait).ok()?;
    decode_visible_state(&frame)
}

fn observe_live_visible_input_proof() -> Result<VisibleState, BootstrapFailure> {
    const OBSERVER_MAX_POLLS: usize = 128;
    const OBSERVER_YIELDS_BETWEEN_POLLS: usize = 4096;
    let mut witness = ProofVisibleInputWitness::new();
    let mut last_state = None;

    for _ in 0..OBSERVER_MAX_POLLS {
        if let Some(state) = fetch_live_visible_state() {
            witness.observe(state);
            let observed_state = witness.observed_state();
            last_state = Some(observed_state);
            if witness.ready() {
                return Ok(observed_state);
            }
        }
        for _ in 0..OBSERVER_YIELDS_BETWEEN_POLLS {
            let _ = yield_();
        }
    }

    if let Some(state) = last_state {
        let _ = debug_println("bootstrap: visible-state timeout");
        emit_missing_visible_input_bits(state);
        return Ok(state);
    }

    Err(BootstrapFailure::VisibleInputEvidence)
}

fn observe_display_bootstrap_state() -> Result<VisibleState, BootstrapFailure> {
    const OBSERVER_MAX_POLLS: usize = 128;
    const OBSERVER_YIELDS_BETWEEN_POLLS: usize = 4096;
    let mut last_state = None;

    for _ in 0..OBSERVER_MAX_POLLS {
        if let Some(state) = fetch_live_visible_state() {
            last_state = Some(state);
            if display_bootstrap_ready(state) {
                return Ok(state);
            }
        }
        for _ in 0..OBSERVER_YIELDS_BETWEEN_POLLS {
            let _ = yield_();
        }
    }

    if let Some(state) = last_state {
        let _ = debug_println(&format!(
            "bootstrap: display-state timeout backend={} scanout={} systemui={}",
            u8::from(state.backend_visible),
            u8::from(state.display_scanout_ready),
            u8::from(state.systemui_first_frame_visible),
        ));
    }

    Err(BootstrapFailure::DisplayServiceEvidence)
}

fn observe_interactive_scene_state() -> Result<VisibleState, BootstrapFailure> {
    const OBSERVER_MAX_POLLS: usize = 128;
    const OBSERVER_YIELDS_BETWEEN_POLLS: usize = 4096;
    let mut last_state = None;

    for _ in 0..OBSERVER_MAX_POLLS {
        if let Some(state) = fetch_live_visible_state() {
            last_state = Some(state);
            if interactive_scene_ready(state) {
                return Ok(state);
            }
        }
        for _ in 0..OBSERVER_YIELDS_BETWEEN_POLLS {
            let _ = yield_();
        }
    }

    if let Some(state) = last_state {
        let _ = debug_println(&format!(
            "bootstrap: interactive-state timeout backend={} scanout={} systemui={} scene={} full={} click_target={} keyboard_target={}",
            u8::from(state.backend_visible),
            u8::from(state.display_scanout_ready),
            u8::from(state.systemui_first_frame_visible),
            u8::from(state.scene_ready),
            u8::from(state.full_window_visible),
            u8::from(state.click_target_visible),
            u8::from(state.keyboard_target_visible),
        ));
    }

    Err(BootstrapFailure::InteractiveSceneEvidence)
}
