// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: the system's own keys — TASK-0066 tiling chords (Super+Ctrl+…) and RFC-0095 capture
//! keys (Print, Shift+Print, Alt+Print): inputd recognizes them on the press as EVENTS for the
//! key channel (`InputDispatch::Chord` / `CaptureKey`, TASK-0348 P17 — one dispatch per press,
//! in order with the keys around it, never a one-shot fact on the sampled visible state), and
//! the key never becomes a keyboard dispatch (imed and the focused app never see it). A key
//! without its modifiers stays an ordinary key. The chords are the shortcut registry's
//! (RFC-0113 §7), matched with EXACTLY their modifiers.
//! OWNERS: @runtime

use hid::TimestampNs;
use hidrawd::{DeviceId, HidrawdService};
use inputd::{InputDispatch, InputdConfig, InputdService};
use windowd::{CallerCtx, CommitSeq, Layer, Rect, SurfaceBuffer, WindowServer, WindowdConfig};

fn fixture_server() -> WindowServer {
    let caller = CallerCtx::from_service_metadata(0x55);
    let mut server =
        WindowServer::new(WindowdConfig { width: 64, height: 48, hz: 60 }).expect("server");
    let buffer =
        SurfaceBuffer::solid(caller, 50, 24, 16, [0x24, 0x28, 0x34, 0xff]).expect("buffer");
    let surface = server.create_surface(caller, buffer.clone()).expect("surface");
    server.queue_buffer(caller, surface, buffer, &[Rect::new(0, 0, 24, 16)]).expect("queue");
    server
        .commit_scene(
            CallerCtx::system(),
            CommitSeq::new(1),
            &[Layer { surface, x: 8, y: 8, z: 0 }],
        )
        .expect("scene");
    let _ack = server.present_tick().expect("present tick").expect("present");
    server
}

fn keyboard_count(d: &[InputDispatch]) -> usize {
    d.iter().filter(|x| matches!(x, InputDispatch::Keyboard { .. })).count()
}

/// The window chord codes a press dispatched, in order (TASK-0348 P17: events, one per press).
fn chord_codes(d: &[InputDispatch]) -> Vec<u8> {
    d.iter()
        .filter_map(|x| match x {
            InputDispatch::Chord { code, .. } => Some(*code),
            _ => None,
        })
        .collect()
}

/// The capture kinds a press dispatched, in order.
fn capture_kinds(d: &[InputDispatch]) -> Vec<u8> {
    d.iter()
        .filter_map(|x| match x {
            InputDispatch::CaptureKey { kind, .. } => Some(*kind),
            _ => None,
        })
        .collect()
}

/// HID boot report: byte 0 = modifier mask (bit0 LCtrl, bit1 LShift, bit2 LAlt, bit3 LGUI).
const LCTRL: u8 = 0x01;
const LSHIFT: u8 = 0x02;
const LALT: u8 = 0x04;
const LGUI: u8 = 0x08;
const LEFT: u8 = 0x50;
const RIGHT: u8 = 0x4f;
const UP: u8 = 0x52;
const DOWN: u8 = 0x51;
const KEY_F: u8 = 0x09;
const PRINT: u8 = 0x46;

struct Rig {
    inputd: InputdService<WindowServer>,
    hidrawd: HidrawdService,
    keyboard: DeviceId,
    t: u64,
}

impl Rig {
    fn new() -> Self {
        let server = fixture_server();
        let config = InputdConfig::new("de", 100, 10, 1, 2, 1, 32, 16, 12, 12).expect("config");
        let mut inputd = InputdService::new(server, config).expect("inputd");
        let mut hidrawd = HidrawdService::new();
        let keyboard = DeviceId::new(9);
        let mouse = DeviceId::new(10);
        hidrawd.register_keyboard(keyboard);
        hidrawd.register_mouse(mouse);
        let click = hidrawd
            .ingest_mouse_report(mouse, TimestampNs::new(1), &[0b001, 0, 0])
            .expect("focus click");
        inputd.apply_hid_batch(&click).expect("focus route");
        Self { inputd, hidrawd, keyboard, t: 100 }
    }

    /// Presses `key` with `mods` held (one report), then releases everything: the press's
    /// dispatches.
    fn press(&mut self, mods: u8, key: u8) -> Vec<InputDispatch> {
        self.t += 100;
        let down = self
            .hidrawd
            .ingest_keyboard_report(
                self.keyboard,
                TimestampNs::new(self.t),
                &[mods, 0, key, 0, 0, 0, 0, 0],
            )
            .expect("down");
        let keys = self.inputd.apply_hid_batch(&down).expect("down dispatches");
        self.t += 100;
        let up = self
            .hidrawd
            .ingest_keyboard_report(
                self.keyboard,
                TimestampNs::new(self.t),
                &[0, 0, 0, 0, 0, 0, 0, 0],
            )
            .expect("up");
        let _ = self.inputd.apply_hid_batch(&up);
        keys
    }

    /// Presses `key` with `mods` held: `(keyboard dispatches, the chord code or 0)`.
    fn chord(&mut self, mods: u8, key: u8) -> (usize, u8) {
        let d = self.press(mods, key);
        let codes = chord_codes(&d);
        assert!(codes.len() <= 1, "one press, at most one chord: {codes:?}");
        (keyboard_count(&d), codes.first().copied().unwrap_or(0))
    }

    /// Presses `key` with `mods` held: `(keyboard dispatches, the capture kind or 0)`.
    fn capture(&mut self, mods: u8, key: u8) -> (usize, u8) {
        let d = self.press(mods, key);
        assert!(chord_codes(&d).is_empty(), "no capture key is a tiling chord");
        let kinds = capture_kinds(&d);
        assert!(kinds.len() <= 1, "one press, at most one capture key: {kinds:?}");
        (keyboard_count(&d), kinds.first().copied().unwrap_or(0))
    }
}

#[test]
fn super_ctrl_chords_become_zone_codes_and_never_keys() {
    let mut rig = Rig::new();
    assert_eq!(rig.chord(LCTRL | LGUI, LEFT), (0, 1), "left half; the key is no keyboard dispatch");
    assert_eq!(rig.chord(LCTRL | LGUI, RIGHT), (0, 2), "right half");
    assert_eq!(rig.chord(LCTRL | LGUI | LSHIFT, LEFT), (0, 5), "top-left quarter");
    assert_eq!(rig.chord(LCTRL | LGUI | LALT, RIGHT), (0, 8), "bottom-right quarter");
    assert_eq!(rig.chord(LCTRL | LGUI, UP), (0, 9), "fill");
    assert_eq!(rig.chord(LCTRL | LGUI, KEY_F), (0, 9), "F = fill");
    assert_eq!(rig.chord(LCTRL | LGUI, DOWN), (0, 10), "return");
    // TASK-0333 6d/6e-1: the letter chords — the board's cycle 73 saw the arrows work and the
    // letters not; the table must dispatch both alike.
    assert_eq!(rig.chord(LCTRL | LGUI, 0x10), (0, 14), "M = minimize, no keyboard dispatch");
    assert_eq!(rig.chord(LCTRL | LGUI, 0x1a), (0, 15), "W = close, no keyboard dispatch");
}

/// TASK-0348 P17: the chord is an event of the dispatch log — carrying its press's stream
/// sequence and time — and leaves with the log; it is never parked for a later state push
/// (the gate of P17: two identical pointer samples around a chord apply it exactly once).
#[test]
fn a_chord_is_one_dispatch_with_its_press_and_rides_no_sample() {
    let mut rig = Rig::new();
    let d = rig.press(LCTRL | LGUI, 0x10);
    let chords: Vec<_> = d
        .iter()
        .filter_map(|x| match x {
            InputDispatch::Chord { code, seq, t_ns } => Some((*code, *seq, *t_ns)),
            _ => None,
        })
        .collect();
    assert_eq!(chords.len(), 1, "one press, one event");
    assert_eq!(chords[0].0, 14, "M = minimize");
    assert_eq!(chords[0].2, 200, "its press report's time");
    assert!(chord_codes(rig.inputd.recent_dispatches()).is_empty(), "taken with the log");
    assert!(chord_codes(&rig.press(0, 0)).is_empty(), "an empty report replays nothing");
}

/// TASK-0348 P7: the keys' witness names a second's modifier mask and chord count and the last
/// chord's report time — never a key (RFC-0113 §6) — so a board log shows the hop a chord lost.
#[test]
fn the_key_witness_counts_modifiers_and_chords_not_keys() {
    let mut rig = Rig::new();
    assert_eq!(rig.inputd.key_witness().take_counts(), (0, 0), "nothing yet");
    assert_eq!(rig.inputd.key_witness().take_chord(), None);
    let _ = rig.chord(LCTRL | LGUI, 0x1a); // W = close
    let _ = rig.chord(LCTRL | LGUI, 0x10); // M = minimize
    let _ = rig.chord(LSHIFT, 0x04); // Shift+A: a key, no chord
    let (mods, chords) = rig.inputd.key_witness().take_counts();
    assert_eq!(chords, 2, "W and M");
    assert_eq!(mods, 0x13, "Shift|Ctrl|Super as the surface wire's bits — no key");
    let (code, report_ns) = rig.inputd.key_witness().take_chord().expect("the last chord");
    assert_eq!(code, 14, "M = minimize was the last chord");
    assert_eq!(report_ns, 400, "its press report's time (the rig's third report)");
    assert_eq!(rig.inputd.key_witness().take_counts(), (0, 0), "taken");
    assert_eq!(rig.inputd.key_witness().take_chord(), None, "once");
}

/// Without Super the same keys are ordinary input: no chord parked, the key dispatches (or is
/// an unproducible Ctrl chord the layout skips) — nothing silently becomes a tile.
#[test]
fn test_reject_chords_without_super() {
    let mut rig = Rig::new();
    assert_eq!(rig.chord(0, 0x04).1, 0, "a letter");
    assert_eq!(rig.chord(LCTRL, LEFT).1, 0, "Ctrl alone is not the tiling modifier");
    assert_eq!(rig.chord(LGUI, LEFT).1, 0, "Super alone is not the tiling modifier");
    assert_eq!(rig.chord(LCTRL | LGUI, 0x04).1, 0, "Super+Ctrl+A is not in the table");
}

/// RFC-0095: Print, Shift+Print and Alt+Print are capture keys — facts for windowd (which hands
/// them to the shell), never keys for imed or an app.
#[test]
fn print_keys_become_capture_facts_and_never_keys() {
    let mut rig = Rig::new();
    assert_eq!(rig.capture(0, PRINT), (0, 1), "the screenshot UI; Print is no keyboard dispatch");
    assert_eq!(rig.capture(LSHIFT, PRINT), (0, 2), "the screen");
    assert_eq!(rig.capture(LALT, PRINT), (0, 3), "the focused window");
}

/// With Ctrl or Super held Print is no capture key (those chords stay free).
#[test]
fn test_reject_print_with_ctrl_or_super() {
    let mut rig = Rig::new();
    assert_eq!(rig.capture(LCTRL, PRINT).1, 0, "Ctrl+Print");
    assert_eq!(rig.capture(LGUI, PRINT).1, 0, "Super+Print");
}

/// RFC-0113 §7: a chord matches only its very modifier set — one more modifier is no tile, no
/// capture, and no stray key either (Ctrl is held: the layout skips it).
#[test]
fn test_reject_a_chord_with_extra_modifiers() {
    let mut rig = Rig::new();
    assert_eq!(
        rig.chord(LCTRL | LGUI | LSHIFT, KEY_F),
        (0, 0),
        "Super+Ctrl+Shift+F: no key, no fill"
    );
    assert_eq!(
        rig.chord(LCTRL | LGUI | LSHIFT | LALT, LEFT).1,
        0,
        "Shift and Alt at once is no quarter"
    );
    assert_eq!(rig.capture(LSHIFT | LALT, PRINT).1, 0, "Shift+Alt+Print is no capture key");
    assert_eq!(rig.chord(LCTRL | LGUI | LSHIFT, UP).1, 0, "Super+Ctrl+Shift+Up is no fill");
}

/// The registry's modifier bits ARE the surface wire's (a tap's modifiers and a chord's are one
/// vocabulary).
#[test]
fn registry_modifier_bits_are_the_wire_bits() {
    use nexus_display_proto::surface_input as wire;
    assert_eq!(shortcuts::Mods::SHIFT.bits(), wire::MOD_SHIFT);
    assert_eq!(shortcuts::Mods::CTRL.bits(), wire::MOD_CTRL);
    assert_eq!(shortcuts::Mods::ALT.bits(), wire::MOD_ALT);
    assert_eq!(shortcuts::Mods::ALTGR.bits(), wire::MOD_ALTGR);
    assert_eq!(shortcuts::Mods::SUPER.bits(), wire::MOD_SUPER);
}
