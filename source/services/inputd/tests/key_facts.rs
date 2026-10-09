// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: one-shot key FACTS, not keys — TASK-0066 tiling chords (Super+Ctrl+…) and RFC-0095
//! capture keys (Print, Shift+Print, Alt+Print): inputd recognizes them on the press, parks the
//! code for exactly one state push (`take_wm_chord`, `take_capture_key`, `stamp_key_facts`),
//! and the key never becomes a keyboard dispatch (imed and the focused app never see it). A key
//! without its modifiers stays an ordinary key.
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

    /// Presses `key` with `mods` held (one report), then releases everything.
    fn chord(&mut self, mods: u8, key: u8) -> usize {
        self.t += 100;
        let down = self
            .hidrawd
            .ingest_keyboard_report(
                self.keyboard,
                TimestampNs::new(self.t),
                &[mods, 0, key, 0, 0, 0, 0, 0],
            )
            .expect("down");
        let keys = keyboard_count(&self.inputd.apply_hid_batch(&down).expect("down dispatches"));
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
}

#[test]
fn super_ctrl_chords_become_zone_codes_and_never_keys() {
    let mut rig = Rig::new();
    assert_eq!(rig.chord(LCTRL | LGUI, LEFT), 0, "the chord's key is no keyboard dispatch");
    assert_eq!(rig.inputd.take_wm_chord(), 1, "left half");
    assert_eq!(rig.inputd.take_wm_chord(), 0, "one-shot");
    assert_eq!(rig.chord(LCTRL | LGUI, RIGHT), 0);
    assert_eq!(rig.inputd.take_wm_chord(), 2, "right half");
    assert_eq!(rig.chord(LCTRL | LGUI | LSHIFT, LEFT), 0);
    assert_eq!(rig.inputd.take_wm_chord(), 5, "top-left quarter");
    assert_eq!(rig.chord(LCTRL | LGUI | LALT, RIGHT), 0);
    assert_eq!(rig.inputd.take_wm_chord(), 8, "bottom-right quarter");
    assert_eq!(rig.chord(LCTRL | LGUI, UP), 0);
    assert_eq!(rig.inputd.take_wm_chord(), 9, "fill");
    assert_eq!(rig.chord(LCTRL | LGUI, KEY_F), 0);
    assert_eq!(rig.inputd.take_wm_chord(), 9, "F = fill");
    assert_eq!(rig.chord(LCTRL | LGUI, DOWN), 0);
    assert_eq!(rig.inputd.take_wm_chord(), 10, "return");
}

/// Without Super the same keys are ordinary input: no chord parked, the key dispatches (or is
/// an unproducible Ctrl chord the layout skips) — nothing silently becomes a tile.
#[test]
fn test_reject_chords_without_super() {
    let mut rig = Rig::new();
    let _ = rig.chord(0, 0x04); // "a"
    assert_eq!(rig.inputd.take_wm_chord(), 0);
    let _ = rig.chord(LCTRL, LEFT);
    assert_eq!(rig.inputd.take_wm_chord(), 0, "Ctrl alone is not the tiling modifier");
    let _ = rig.chord(LGUI, LEFT);
    assert_eq!(rig.inputd.take_wm_chord(), 0, "Super alone is not the tiling modifier");
    let _ = rig.chord(LCTRL | LGUI, 0x04);
    assert_eq!(rig.inputd.take_wm_chord(), 0, "Super+Ctrl+A is not in the table");
}

/// RFC-0095: Print, Shift+Print and Alt+Print are capture keys — facts for windowd (which hands
/// them to the shell), never keys for imed or an app.
#[test]
fn print_keys_become_capture_facts_and_never_keys() {
    let mut rig = Rig::new();
    assert_eq!(rig.chord(0, PRINT), 0, "Print is no keyboard dispatch");
    assert_eq!(rig.inputd.take_capture_key(), 1, "the screenshot UI");
    assert_eq!(rig.inputd.take_capture_key(), 0, "one-shot");
    assert_eq!(rig.chord(LSHIFT, PRINT), 0);
    assert_eq!(rig.inputd.take_capture_key(), 2, "the screen");
    assert_eq!(rig.chord(LALT, PRINT), 0);
    assert_eq!(rig.inputd.take_capture_key(), 3, "the focused window");
    assert_eq!(rig.inputd.take_wm_chord(), 0, "no capture key is a tiling chord");
}

/// With Ctrl or Super held Print is no capture key (those chords stay free).
#[test]
fn test_reject_print_with_ctrl_or_super() {
    let mut rig = Rig::new();
    let _ = rig.chord(LCTRL, PRINT);
    assert_eq!(rig.inputd.take_capture_key(), 0, "Ctrl+Print");
    let _ = rig.chord(LGUI, PRINT);
    assert_eq!(rig.inputd.take_capture_key(), 0, "Super+Print");
}

/// The push stamps each new fact once; a field without a new fact keeps one not yet delivered.
#[test]
fn key_facts_stamp_once_and_keep_an_undelivered_fact() {
    let mut rig = Rig::new();
    let _ = rig.chord(LCTRL | LGUI, LEFT);
    let (mut chord, mut capture) = (0u8, 3u8);
    assert!(rig.inputd.stamp_key_facts(&mut chord, &mut capture), "a new chord");
    assert_eq!((chord, capture), (1, 3), "the chord lands, the undelivered capture key stays");
    assert!(!rig.inputd.stamp_key_facts(&mut chord, &mut capture), "nothing new: no push owed");
}
