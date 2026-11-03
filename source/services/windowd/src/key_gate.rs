// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: windowd's key-route gate (RFC-0113 §3, ADR-0090), pure: who may send keys, where a
//! key goes, which session the input method must hold, which of its pushes a surface takes. The
//! runtime (`compositor/runtime/key_input.rs`, `text_input.rs`) asks it; the host tests prove its
//! refusals. Identity is the kernel's sender id — never a payload field.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: inline + `tests/key_gate.rs` (`test_reject_*` for every refusal)

/// The input method's field kind for "no field, the window".
pub use nexus_wire::imed::FIELD_KIND_NONE;

/// Why a key or a push was not delivered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The frame came from a service that does not own this channel.
    ForeignSender,
    /// No window holds focus: there is nobody to type into.
    NoWindowFocus,
    /// The push names a surface that is not the holder (a stale focus).
    StaleSurface,
    /// Without a field, the window takes actions only (Escape) — never text.
    TextWithoutField,
}

/// The key channel's one sender is inputd.
pub fn key_sender(sender_sid: u64, inputd_sid: u64) -> Result<(), Refusal> {
    if sender_sid == inputd_sid && inputd_sid != 0 {
        Ok(())
    } else {
        Err(Refusal::ForeignSender)
    }
}

/// A layout source's text or action goes to the surface that holds window focus; its app routes
/// it to its focused field, in the order its own events arrive.
pub fn key_target<T>(window_focus: Option<T>) -> Result<T, Refusal> {
    window_focus.ok_or(Refusal::NoWindowFocus)
}

/// The session the input method must hold for a composition key: the focused field's
/// `(surface, kind)`, else the focused window's with [`FIELD_KIND_NONE`]; `None` = nobody.
#[must_use]
pub fn compose_session(field: Option<(u32, u8)>, window: Option<u32>) -> Option<(u32, u8)> {
    field.or_else(|| window.map(|surface| (surface, FIELD_KIND_NONE)))
}

/// What windowd sends the input method before a key, to move its session from `open` to `want`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionStep {
    /// The session is the one wanted.
    Keep,
    /// Close `close` (if a session was open), open `open` — then the key.
    Switch { close: Option<(u32, u8)>, open: (u32, u8) },
}

#[must_use]
pub fn session_step(open: Option<(u32, u8)>, want: (u32, u8)) -> SessionStep {
    if open == Some(want) {
        SessionStep::Keep
    } else {
        SessionStep::Switch { close: open, open: want }
    }
}

/// Who holds the input method's output right now.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holders {
    /// The surface whose text field is focused.
    pub field: Option<u32>,
    /// The surface that holds window focus for the input method (no field focused).
    pub window: Option<u32>,
}

/// The input method's pushes come from imed only.
pub fn imed_sender(sender_sid: u64, imed_sid: u64) -> Result<(), Refusal> {
    if sender_sid == imed_sid && imed_sid != 0 {
        Ok(())
    } else {
        Err(Refusal::ForeignSender)
    }
}

/// The holder an input-method push for `surface` reaches (`is_action`: an action, not text): the
/// field holder; without a field, the window holder and an action only; else refused.
pub fn imed_push_holder(surface: u64, is_action: bool, holders: Holders) -> Result<u32, Refusal> {
    if let Some(field) = holders.field {
        return if u64::from(field) == surface { Ok(field) } else { Err(Refusal::StaleSurface) };
    }
    match holders.window {
        Some(window) if u64::from(window) == surface => {
            if is_action {
                Ok(window)
            } else {
                Err(Refusal::TextWithoutField)
            }
        }
        _ => Err(Refusal::StaleSurface),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_follows_the_field_then_the_window() {
        assert_eq!(compose_session(Some((7, 0)), Some(3)), Some((7, 0)));
        assert_eq!(compose_session(None, Some(3)), Some((3, FIELD_KIND_NONE)));
        assert_eq!(compose_session(None, None), None);
        assert_eq!(session_step(Some((7, 0)), (7, 0)), SessionStep::Keep);
        assert_eq!(
            session_step(Some((3, FIELD_KIND_NONE)), (7, 0)),
            SessionStep::Switch { close: Some((3, FIELD_KIND_NONE)), open: (7, 0) }
        );
    }
}

/// The system's own keys on the key channel (TASK-0348 P17): a window chord of the shortcut
/// registry or a capture key, each an EVENT applied once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SystemKey {
    /// windowd's `zones::CODE_*` for the focused window.
    Chord(u8),
    /// RFC-0095: 1 the screenshot UI, 2 the screen, 3 the focused window — for the shell.
    Capture(u8),
}

/// Classifies a key-channel entry by its kind and value: a chord carries a code (1..=255), a
/// capture key one of its three kinds; anything else — a text or action key, an unknown kind, a
/// value past its range — is `None` (fail-closed: the channel's decode refuses it before this).
#[must_use]
pub fn system_key(kind: u8, value: u32) -> Option<SystemKey> {
    use input_live_protocol::{
        CAPTURE_KIND_UI, CAPTURE_KIND_WINDOW, KEY_INPUT_CAPTURE, KEY_INPUT_CHORD,
    };
    let byte = u8::try_from(value).ok().filter(|v| *v != 0)?;
    match kind {
        KEY_INPUT_CHORD => Some(SystemKey::Chord(byte)),
        KEY_INPUT_CAPTURE if (CAPTURE_KIND_UI..=CAPTURE_KIND_WINDOW).contains(&value) => {
            Some(SystemKey::Capture(byte))
        }
        _ => None,
    }
}
