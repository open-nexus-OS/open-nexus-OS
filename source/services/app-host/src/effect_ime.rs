// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `svc.ime.*` effect methods (RFC-0075 Phase 2) — OSK key/action
//! injection through imed's DEDICATED osk endpoint (route slot 18 =
//! the authorization; only ime-type bundles hold it). Fire-and-forget
//! NONBLOCK sends; imed replies only to reply-cap probes.
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: QEMU `SELFTEST: ime v2 osk ok` + the interactive OSK proof.

use crate::effect_host::{
    raw_marker, AppEffectHost, ERR_SVC_SHAPE, ERR_SVC_UNAVAILABLE, ERR_SVC_UNKNOWN,
};
use nexus_dsl_runtime::Value;

impl AppEffectHost {
    /// `svc.ime.key(text)` → OSK key injection (RFC-0075 Phase 2): ONE
    /// character to imed's DEDICATED osk endpoint (route slot = the
    /// authorization; only ime-type bundles hold it). Fire-and-forget
    /// NONBLOCK — key delivery is push-shaped; imed replies only to probes.
    pub(crate) fn ime_key(&self, text: &str) -> Result<Value, u32> {
        let ch = text.chars().next().ok_or(ERR_SVC_SHAPE)?;
        let frame = nexus_wire::imed::encode_key(
            nexus_wire::imed::KEY_SOURCE_OSK,
            nexus_wire::imed::KEY_KIND_TEXT,
            u32::from(ch),
            0,
            0,
        );
        self.ime_send(&frame)
    }

    /// `svc.ime.action(name)` → OSK control action ("backspace" | "enter").
    pub(crate) fn ime_action(&self, action: &str) -> Result<Value, u32> {
        let code = match action {
            "backspace" => nexus_wire::imed::ACTION_BACKSPACE,
            "enter" => nexus_wire::imed::ACTION_ENTER,
            _ => return Err(ERR_SVC_SHAPE),
        };
        let frame = nexus_wire::imed::encode_key(
            nexus_wire::imed::KEY_SOURCE_OSK,
            nexus_wire::imed::KEY_KIND_ACTION,
            0,
            code,
            0,
        );
        self.ime_send(&frame)
    }

    /// `svc.ime.select(i)` → commits candidate `i` of the current page.
    pub(crate) fn ime_select(&self, index: i64) -> Result<Value, u32> {
        let index = u8::try_from(index).map_err(|_| ERR_SVC_SHAPE)?;
        let frame = nexus_wire::imed::encode_candidate_select(index);
        self.ime_send(&frame)
    }

    /// `svc.ime.layout(tag)` → switches the composition engine (globe key).
    pub(crate) fn ime_layout(&self, layout: &str) -> Result<Value, u32> {
        let mut buf = [0u8; 16];
        let n = nexus_wire::imed::encode_set_layout(layout, &mut buf).ok_or(ERR_SVC_SHAPE)?;
        self.ime_send(&buf[..n])
    }

    /// `svc.ime.cycle(current)` → switches to the platform's NEXT layout
    /// (cycle order = the keymaps SSOT — never an app-side if-chain). The
    /// switch is SYSTEM-WIDE: imed persists `input.keymap` and the change
    /// returns as a `KeymapEvent` region push.
    pub(crate) fn ime_cycle(&self, current: &str) -> Result<Value, u32> {
        const ORDER: &[&str] = &["de", "us", "jp", "kr", "zh"];
        let idx = ORDER.iter().position(|l| *l == current).unwrap_or(0);
        let next = ORDER[(idx + 1) % ORDER.len()];
        self.ime_layout(next)
    }

    /// `svc.ime.rows(layout, row)` → the OSK row DATA from the keymaps SSOT
    /// (RFC-0075 Phase 8b): `List<OskKey{label,key,action}>`. Unknown layout
    /// or row = empty list (the app's `List` renders nothing — bounded,
    /// never an error popup for data the platform simply lacks).
    pub(crate) fn ime_rows(&self, layout: &str, row: i64) -> Result<Value, u32> {
        use alloc::string::String;
        let (Some(label_sym), Some(key_sym), Some(action_sym)) =
            (self.label_sym, self.key_sym, self.action_sym)
        else {
            return Err(ERR_SVC_SHAPE); // program never reads the fields
        };
        let Ok(layout) = keymaps::LayoutId::try_from(layout) else {
            return Ok(Value::List(alloc::vec::Vec::new()));
        };
        let Ok(row) = usize::try_from(row) else {
            return Ok(Value::List(alloc::vec::Vec::new()));
        };
        let rows: alloc::vec::Vec<Value> = keymaps::osk_rows(layout, row)
            .iter()
            .map(|k| {
                let mut fields = alloc::vec![
                    (label_sym, Value::Str(String::from(k.label))),
                    (key_sym, Value::Str(String::from(k.key))),
                    (action_sym, Value::Str(String::from(k.action))),
                ];
                fields.sort_by_key(|(sym, _)| *sym);
                Value::Record(fields)
            })
            .collect();
        Ok(Value::List(rows))
    }

    pub(crate) fn ime_send(&self, frame: &[u8]) -> Result<Value, u32> {
        let send_slot = Self::svc_send_slot("ime").ok_or(ERR_SVC_UNKNOWN)?;
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        let ok =
            nexus_abi::ipc_send_v1(send_slot, &hdr, frame, nexus_abi::IPC_SYS_NONBLOCK, 0).is_ok();
        if !ok {
            raw_marker("apphost: dsl svc ime FAIL (send)");
        }
        Ok(Value::Bool(ok))
    }

    /// Sends a presentation control to windowd on the surface request channel
    /// (fire-and-forget NONBLOCK; windowd pushes the resulting theme/profile
    /// back over the event channel, which re-mounts the view).
    pub(crate) fn presentation_control(&self, key: &str, value: &str) -> Result<Value, u32> {
        use nexus_abi::settingsd as sw;
        use nexus_display_proto::client_surface as wire;
        let (control, v) = if key == sw::KEY_UI_THEME_MODE {
            let v = if value == "light" { wire::THEME_LIGHT } else { wire::THEME_DARK };
            (wire::CONTROL_THEME, v)
        } else if key == sw::KEY_UI_THEME_ACCENT {
            // Accent-palette pick: name → index (unknown names fail closed —
            // settingsd would refuse them too; the palette is the SSOT).
            let Some(idx) = nexus_dsl_runtime::theme_tokens::accent_index(value) else {
                raw_marker("apphost: dsl svc settings.set FAIL (accent name)");
                return Err(ERR_SVC_UNAVAILABLE);
            };
            (wire::CONTROL_THEME_ACCENT, idx)
        } else if key == "window.control" {
            // App-chrome window controls (the window-kit app menu). The recv
            // path carries no sender identity, so the value byte names the
            // caller's own surface: minimize/close = the surface id; mode =
            // `id << 4 | WIN_MODE_*` (ids and modes are both < 16).
            // RECORDED FOLLOW-UP (same class as the CONTROL sender-role
            // check): a client could name a foreign id — presentation-only
            // blast radius until per-sender identity lands.
            let sid = (self.surface_id & 0x0F) as u8;
            if sid == 0 {
                raw_marker("apphost: dsl svc settings.set FAIL (no surface id)");
                return Err(ERR_SVC_UNAVAILABLE);
            }
            match value {
                "minimize" => (wire::CONTROL_WIN_MINIMIZE, sid),
                "close" => (wire::CONTROL_WIN_CLOSE, sid),
                // zoom / mode.*: one MODE control; AUTO = toggle fullscreen.
                "zoom" => (wire::CONTROL_WIN_MODE, sid << 4 | wire::WIN_MODE_AUTO),
                "mode.fullscreen" => (wire::CONTROL_WIN_MODE, sid << 4 | wire::WIN_MODE_FULLSCREEN),
                "mode.freeform" => (wire::CONTROL_WIN_MODE, sid << 4 | wire::WIN_MODE_FREEFORM),
                "mode.split" => (wire::CONTROL_WIN_MODE, sid << 4 | wire::WIN_MODE_SPLIT),
                _ => {
                    raw_marker("apphost: dsl svc settings.set FAIL (window control)");
                    return Err(ERR_SVC_UNAVAILABLE);
                }
            }
        } else {
            let v = if value == "desktop" { wire::PROFILE_DESKTOP } else { wire::PROFILE_TABLET };
            (wire::CONTROL_SHELL_PROFILE, v)
        };
        let frame = wire::encode_surface_control(control, v);
        // The windowd surface request slot (main.rs WINDOWD_SEND_SLOT).
        const WINDOWD_SEND_SLOT: u32 = 5;
        let hdr = nexus_abi::MsgHeader::new(0, 0, 0, 0, frame.len() as u32);
        match nexus_abi::ipc_send_v1(
            WINDOWD_SEND_SLOT,
            &hdr,
            &frame,
            nexus_abi::IPC_SYS_NONBLOCK,
            0,
        ) {
            Ok(_) => {
                raw_marker("apphost: dsl svc settings.set control ok");
                Ok(Value::Bool(true))
            }
            Err(_) => {
                raw_marker("apphost: dsl svc settings.set control FAIL (send)");
                Err(ERR_SVC_UNAVAILABLE)
            }
        }
    }
}
