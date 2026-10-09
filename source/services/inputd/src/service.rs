// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Bounded `inputd` merge/config/route core for TASK-0253.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 host contract tests in the `inputd` crate.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

extern crate alloc;

use crate::config::InputdConfig;
use crate::key_facts::{capture_key_for, is_modifier, wm_chord_for, ModifierState};
use crate::route::RouteTarget;
use crate::{ImeHook, InputDispatch, InputdError};
use alloc::vec::Vec;
use hid::{AbsoluteAxis, HidEvent, HidEventKind, KeyboardUsage, RelativeAxis};
use hidrawd::{HidBatch, HidDeviceKind, PointerSource};
use key_repeat::{MonotonicNs, RepeatEngine, RepeatKey};
use keymaps::{KeyAction, KeyOutput, Keymap, LayoutId};
use pointer_accel::PointerAccel;
use pointer_state::{PointerPosition, PointerSpace, PointerState, PointerTransform};
use touch::{TouchEvent, TouchPhase};

pub struct InputdService<R> {
    router: R,
    layout: LayoutId,
    keymap: Keymap,
    repeat: RepeatEngine,
    pointer_accel: PointerAccel,
    queue_capacity: usize,
    dispatch_log: Vec<InputDispatch>,
    pointer_state: PointerState,
    pointer_transform: PointerTransform,
    active_pointer_source: Option<PointerSource>,
    /// The relative pointer travel since last taken (|dx|, |dy| summed, batches with motion):
    /// the board's rate line prints it — aggregates only, never a position.
    travel: (u64, u64, u32),
    primary_pointer_held: bool,
    held_non_modifier_keys: [bool; 256],
    held_non_modifier_key_count: usize,
    /// A tiling chord recognized since the last state push (`zones::CODE_*`, 0 = none).
    pending_wm_chord: u8,
    /// A capture key recognized since the last state push (RFC-0095: 1 Print, 2 Shift+Print,
    /// 3 Alt+Print; 0 = none).
    pending_capture: u8,
    modifiers: ModifierState,
    text_focus: bool,
    ime_visible: bool,
}

impl<R: RouteTarget> InputdService<R> {
    pub fn new(router: R, config: InputdConfig) -> Result<Self, InputdError> {
        let (route_width, route_height) = router.bounds();
        let route_space =
            PointerSpace::new(route_width, route_height).map_err(InputdError::from)?;
        let display_space = config.display_space().unwrap_or(route_space);
        let pointer_transform =
            PointerTransform::new(display_space, route_space).map_err(InputdError::from)?;
        let pointer = config.initial_pointer();
        let pointer_state =
            PointerState::new(display_space, PointerPosition::new(pointer.x(), pointer.y()))
                .map_err(|err| match err {
                    pointer_state::PointerStateError::InitialPositionOutOfBounds { x, y } => {
                        InputdError::InitialPointerOutOfBounds { x, y }
                    }
                    other => InputdError::from(other),
                })?;

        Ok(Self {
            router,
            layout: config.layout(),
            keymap: Keymap::new(config.layout()),
            repeat: RepeatEngine::new(config.repeat()),
            pointer_accel: PointerAccel::new(config.pointer_accel()).map_err(InputdError::from)?,
            queue_capacity: config.queue_capacity().raw(),
            dispatch_log: Vec::new(),
            pointer_state,
            pointer_transform,
            active_pointer_source: None,
            travel: (0, 0, 0),
            primary_pointer_held: false,
            held_non_modifier_keys: [false; 256],
            held_non_modifier_key_count: 0,
            pending_wm_chord: 0,
            pending_capture: 0,
            modifiers: ModifierState::default(),
            text_focus: false,
            ime_visible: false,
        })
    }

    #[must_use]
    pub fn router(&self) -> &R {
        &self.router
    }

    pub fn router_mut(&mut self) -> &mut R {
        &mut self.router
    }

    #[must_use]
    pub fn recent_dispatches(&self) -> &[InputDispatch] {
        self.dispatch_log.as_slice()
    }

    pub fn take_dispatches(&mut self) -> Vec<InputDispatch> {
        core::mem::take(&mut self.dispatch_log)
    }

    pub fn clear_dispatches(&mut self) {
        self.dispatch_log.clear();
    }

    #[must_use]
    /// The relative pointer travel since last taken: (Σ|dx|, Σ|dy|, batches with motion).
    pub fn take_travel(&mut self) -> (u64, u64, u32) {
        core::mem::take(&mut self.travel)
    }

    pub fn display_pointer_position(&self) -> PointerPosition {
        self.pointer_state.display_position()
    }

    #[must_use]
    pub fn route_pointer_position(&self) -> PointerPosition {
        self.pointer_state.route_position(self.pointer_transform)
    }

    #[must_use]
    pub fn display_space(&self) -> PointerSpace {
        self.pointer_state.display_space()
    }

    #[must_use]
    pub fn pointer_transform(&self) -> PointerTransform {
        self.pointer_transform
    }

    #[must_use]
    pub const fn active_pointer_source(&self) -> Option<PointerSource> {
        self.active_pointer_source
    }

    #[must_use]
    pub const fn primary_pointer_held(&self) -> bool {
        self.primary_pointer_held
    }

    #[must_use]
    pub const fn held_non_modifier_key_count(&self) -> usize {
        self.held_non_modifier_key_count
    }

    /// The tiling chord recognized since the last call (one-shot), 0 = none.
    pub fn take_wm_chord(&mut self) -> u8 {
        core::mem::take(&mut self.pending_wm_chord)
    }

    /// The capture key recognized since the last call (one-shot, RFC-0095), 0 = none.
    pub fn take_capture_key(&mut self) -> u8 {
        core::mem::take(&mut self.pending_capture)
    }

    /// Writes this push's one-shot key facts — a tiling chord, a capture key — into the visible
    /// state's fields (a field without a new fact keeps an undelivered one); `true` when there
    /// is one, so the push goes out at once.
    pub fn stamp_key_facts(&mut self, wm_chord: &mut u8, capture: &mut u8) -> bool {
        let (chord, key) = (self.take_wm_chord(), self.take_capture_key());
        if chord != 0 {
            *wm_chord = chord;
        }
        if key != 0 {
            *capture = key;
        }
        chord != 0 || key != 0
    }

    #[must_use]
    pub const fn layout(&self) -> LayoutId {
        self.layout
    }

    #[must_use]
    pub const fn layout_name(&self) -> &'static str {
        match self.layout {
            LayoutId::Us => "us",
            LayoutId::De => "de",
            LayoutId::Jp => "jp",
            LayoutId::Kr => "kr",
            LayoutId::Zh => "zh",
        }
    }

    pub fn set_layout_name(&mut self, name: &str) -> Result<(), InputdError> {
        let layout = LayoutId::try_from(name).map_err(InputdError::from)?;
        self.layout = layout;
        self.keymap = Keymap::new(layout);
        Ok(())
    }

    /// Re-base the pointer DISPLAY space on the compositor's resolved device
    /// mode (boot-time: windowd answers `OP_GET_VISIBLE_MODE` once gpud told
    /// it the real resolution). Rebuilds the display→route transform and
    /// carries the current pointer position over proportionally, so absolute
    /// (tablet/touch) coordinates land in the same space windowd hit-tests in.
    pub fn set_display_space(&mut self, width: u32, height: u32) -> Result<(), InputdError> {
        let old = self.pointer_state.display_space();
        if old.width() == width && old.height() == height {
            return Ok(());
        }
        let display_space = PointerSpace::new(width, height).map_err(InputdError::from)?;
        let (route_width, route_height) = self.router.bounds();
        let route_space =
            PointerSpace::new(route_width, route_height).map_err(InputdError::from)?;
        self.pointer_transform =
            PointerTransform::new(display_space, route_space).map_err(InputdError::from)?;
        let pos = self.pointer_state.display_position();
        let scaled = PointerPosition::new(
            (i64::from(pos.x) * i64::from(width) / i64::from(old.width().max(1))) as i32,
            (i64::from(pos.y) * i64::from(height) / i64::from(old.height().max(1))) as i32,
        );
        self.pointer_state = PointerState::new(display_space, scaled).map_err(InputdError::from)?;
        Ok(())
    }

    pub fn set_text_focus(&mut self, focused: bool) -> Result<Option<ImeHook>, InputdError> {
        self.text_focus = focused;
        if focused && !self.ime_visible {
            self.ime_visible = true;
            self.push_dispatch(InputDispatch::ImeHook(ImeHook::Show))?;
            return Ok(Some(ImeHook::Show));
        }
        if !focused && self.ime_visible {
            self.ime_visible = false;
            self.push_dispatch(InputDispatch::ImeHook(ImeHook::Hide))?;
            return Ok(Some(ImeHook::Hide));
        }
        Ok(None)
    }

    pub fn apply_hid_batch(&mut self, batch: &HidBatch) -> Result<Vec<InputDispatch>, InputdError> {
        self.apply_hid_batch_in_place(batch)?;
        Ok(self.dispatch_log.clone())
    }

    pub fn apply_hid_batch_in_place(&mut self, batch: &HidBatch) -> Result<(), InputdError> {
        self.dispatch_log.clear();
        match batch.kind() {
            HidDeviceKind::Keyboard => self.apply_keyboard(batch.events()),
            HidDeviceKind::Mouse => self.apply_mouse(batch.pointer_source(), batch.events()),
        }
    }

    pub fn apply_touch_event(&mut self, event: TouchEvent) -> Result<InputDispatch, InputdError> {
        let x = i32::try_from(event.x().raw()).map_err(|_| InputdError::PointerOutOfBounds {
            x: i32::MAX,
            y: i32::try_from(event.y().raw()).unwrap_or(i32::MAX),
        })?;
        let y = i32::try_from(event.y().raw())
            .map_err(|_| InputdError::PointerOutOfBounds { x, y: i32::MAX })?;
        self.validate_pointer_bounds(x, y)?;
        let phase = match event.phase() {
            TouchPhase::Down => windowd::TouchInputPhase::Down,
            TouchPhase::Move => windowd::TouchInputPhase::Move,
            TouchPhase::Up => windowd::TouchInputPhase::Up,
        };
        let delivery = self.router.route_touch(x, y, phase).map_err(InputdError::from)?;
        let dispatch = InputDispatch::Touch { delivery, event, x, y };
        self.push_dispatch(dispatch.clone())?;
        Ok(dispatch)
    }

    pub fn tick_repeat(&mut self, now_ns: u64) -> Result<Vec<InputDispatch>, InputdError> {
        let mut out = Vec::new();
        for event in self.repeat.tick(MonotonicNs::new(now_ns)).map_err(InputdError::from)? {
            let usage = KeyboardUsage::from_raw(event.key().raw() as u8);
            let output =
                self.keymap.resolve(usage, self.modifiers.snapshot()).map_err(InputdError::from)?;
            let delivery = self
                .router
                .route_keyboard(u32::from(event.key().raw()))
                .map_err(InputdError::from)?;
            let dispatch = InputDispatch::Keyboard {
                delivery,
                key_code: u32::from(event.key().raw()),
                output,
                repeated: true,
            };
            self.push_dispatch(dispatch.clone())?;
            out.push(dispatch);
        }
        Ok(out)
    }

    fn apply_keyboard(&mut self, events: &[HidEvent]) -> Result<(), InputdError> {
        for event in events {
            if event.kind() != HidEventKind::Key {
                continue;
            }
            let usage = KeyboardUsage::from_raw(event.code().raw() as u8);
            let pressed = event.value().raw() > 0;
            self.modifiers.apply_key(usage, pressed);
            if is_modifier(usage) {
                continue;
            }
            self.update_non_modifier_key_hold(usage, pressed);
            // TASK-0066 window-tiling chords (Super+Ctrl+…) and RFC-0095 capture keys
            // (Print, Shift+Print, Alt+Print): facts, not keys — they ride the state push to
            // windowd and never reach imed or an app. Recognized on the press only.
            if pressed {
                if let Some(code) = wm_chord_for(usage, &self.modifiers) {
                    self.pending_wm_chord = code;
                    continue;
                }
                if let Some(kind) = capture_key_for(usage, &self.modifiers) {
                    self.pending_capture = kind;
                    continue;
                }
            }
            if pressed {
                // Per-EVENT resilience: fast typing packs several keys into
                // one batch (chunked hidraw drains), and any `?` here threw
                // the WHOLE batch away (STATUS_OVERFLOW) — one Ctrl-chord,
                // unmapped usage, or non-monotonic hidraw timestamp silently
                // ate every other key ("fast typing loses input"). A key the
                // layout cannot produce skips ITS event only; repeat arming
                // is best-effort (it re-arms on the next press).
                let Ok(output) = self.keymap.resolve(usage, self.modifiers.snapshot()) else {
                    continue;
                };
                let delivery = self
                    .router
                    .route_keyboard(u32::from(event.code().raw()))
                    .map_err(InputdError::from)?;
                let Ok(repeat_key) = RepeatKey::new(event.code().raw()) else {
                    continue;
                };
                let _ = self.repeat.press(repeat_key, MonotonicNs::new(event.timestamp().raw()));
                let dispatch = InputDispatch::Keyboard {
                    delivery,
                    key_code: u32::from(event.code().raw()),
                    output,
                    repeated: false,
                };
                self.push_dispatch(dispatch.clone())?;
                if self.text_focus
                    && matches!(
                        output,
                        KeyOutput::Text(_) | KeyOutput::Action(KeyAction::ImeSwitch)
                    )
                    && !self.ime_visible
                {
                    self.ime_visible = true;
                    let hook = InputDispatch::ImeHook(ImeHook::Show);
                    self.push_dispatch(hook.clone())?;
                }
            } else if let Ok(repeat_key) = RepeatKey::new(event.code().raw()) {
                self.repeat.release(repeat_key);
            }
        }
        Ok(())
    }

    fn apply_mouse(
        &mut self,
        batch_source: Option<PointerSource>,
        events: &[HidEvent],
    ) -> Result<(), InputdError> {
        let pointer_source = batch_source.or_else(|| infer_pointer_source(events));
        let mut dx = 0;
        let mut dy = 0;
        let mut wheel_delta = 0;
        let mut absolute_x = None;
        let mut absolute_y = None;
        let mut pointer_button_state = None;
        for event in events {
            match event.kind() {
                HidEventKind::Rel if event.code().raw() == RelativeAxis::X.event_code() => {
                    dx += event.value().raw();
                }
                HidEventKind::Rel if event.code().raw() == RelativeAxis::Y.event_code() => {
                    dy += event.value().raw();
                }
                HidEventKind::Rel if event.code().raw() == RelativeAxis::Wheel.event_code() => {
                    wheel_delta += event.value().raw();
                }
                HidEventKind::Abs if event.code().raw() == AbsoluteAxis::X.event_code() => {
                    absolute_x = Some(event.value().raw());
                }
                HidEventKind::Abs if event.code().raw() == AbsoluteAxis::Y.event_code() => {
                    absolute_y = Some(event.value().raw());
                }
                HidEventKind::Btn if event.code().raw() == 0x110 => {
                    pointer_button_state = Some(event.value().raw() > 0);
                }
                _ => {}
            }
        }
        let pointer_down = if let Some(next_state) = pointer_button_state {
            let pressed_now = next_state && !self.primary_pointer_held;
            self.primary_pointer_held = next_state;
            pressed_now
        } else {
            false
        };

        // windowd hit-tests in display space (see `windowd::interaction`: "ships a
        // display-space pointer", "every rect is in display pixels"). Ship the
        // full-resolution display position — NOT the coarse 64×48 route cell. The old
        // `display_to_route` quantization froze the cursor within a ~20px cell and
        // jumped it a whole cell at the boundary (the "bewegt sich nicht / springt" bug).
        if absolute_x.is_some() || absolute_y.is_some() {
            let display = self.pointer_state.apply_absolute(absolute_x, absolute_y);
            let delivery = self
                .router
                .try_coalesce_pointer_move(display.x, display.y)
                .map_err(InputdError::from)?;
            let dispatch = InputDispatch::PointerMove { delivery, x: display.x, y: display.y };
            self.push_dispatch(dispatch.clone())?;
            self.active_pointer_source = pointer_source.or(Some(PointerSource::TabletAbsolute));
        } else if dx != 0 || dy != 0 {
            if pointer_source == Some(PointerSource::MouseRelative)
                && self.relative_motion_blocked_by_absolute_source()
            {
                return self.finish_pointer_side_effects(pointer_down, wheel_delta);
            }
            self.travel.0 = self.travel.0.saturating_add(dx.unsigned_abs().into());
            self.travel.1 = self.travel.1.saturating_add(dy.unsigned_abs().into());
            self.travel.2 = self.travel.2.saturating_add(1);
            let display = self.pointer_state.apply_relative(
                self.pointer_accel.apply_axis(dx).map_err(InputdError::from)?,
                self.pointer_accel.apply_axis(dy).map_err(InputdError::from)?,
            );
            let delivery = self
                .router
                .try_coalesce_pointer_move(display.x, display.y)
                .map_err(InputdError::from)?;
            let dispatch = InputDispatch::PointerMove { delivery, x: display.x, y: display.y };
            self.push_dispatch(dispatch.clone())?;
            self.active_pointer_source = Some(PointerSource::MouseRelative);
        }

        self.finish_pointer_side_effects(pointer_down, wheel_delta)
    }

    fn validate_pointer_bounds(&self, x: i32, y: i32) -> Result<(), InputdError> {
        let (width, height) = self.router.bounds();
        if x < 0
            || y < 0
            || u32::try_from(x).ok().is_none_or(|value| value >= width)
            || u32::try_from(y).ok().is_none_or(|value| value >= height)
        {
            return Err(InputdError::PointerOutOfBounds { x, y });
        }
        Ok(())
    }

    fn push_dispatch(&mut self, dispatch: InputDispatch) -> Result<(), InputdError> {
        if self.dispatch_log.len() >= self.queue_capacity {
            return Err(InputdError::QueueOverflow { capacity: self.queue_capacity });
        }
        self.dispatch_log.push(dispatch);
        Ok(())
    }

    fn finish_pointer_side_effects(
        &mut self,
        pointer_down: bool,
        wheel_delta: i32,
    ) -> Result<(), InputdError> {
        if pointer_down {
            // Display space — same rationale as pointer-move: windowd hit-tests clicks
            // against its display-pixel rects.
            let display = self.pointer_state.display_position();
            self.validate_pointer_bounds(display.x, display.y)?;
            let delivery =
                self.router.route_pointer_down(display.x, display.y).map_err(InputdError::from)?;
            let dispatch = InputDispatch::PointerDown { delivery, x: display.x, y: display.y };
            self.push_dispatch(dispatch.clone())?;
        }
        if wheel_delta != 0 {
            self.push_dispatch(InputDispatch::PointerWheel { delta_y: wheel_delta })?;
            self.active_pointer_source = Some(PointerSource::MouseRelative);
        }
        Ok(())
    }

    fn relative_motion_blocked_by_absolute_source(&self) -> bool {
        matches!(
            self.active_pointer_source,
            Some(PointerSource::TabletAbsolute) | Some(PointerSource::TouchAbsolute)
        )
    }

    fn update_non_modifier_key_hold(&mut self, usage: KeyboardUsage, pressed: bool) {
        let index = usize::from(usage.raw());
        let held = &mut self.held_non_modifier_keys[index];
        match (pressed, *held) {
            (true, false) => {
                *held = true;
                self.held_non_modifier_key_count =
                    self.held_non_modifier_key_count.saturating_add(1);
            }
            (false, true) => {
                *held = false;
                self.held_non_modifier_key_count =
                    self.held_non_modifier_key_count.saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn infer_pointer_source(events: &[HidEvent]) -> Option<PointerSource> {
    if events.iter().any(|event| matches!(event.kind(), HidEventKind::Abs)) {
        return Some(PointerSource::TabletAbsolute);
    }
    if events.iter().any(|event| matches!(event.kind(), HidEventKind::Rel | HidEventKind::Btn)) {
        return Some(PointerSource::MouseRelative);
    }
    None
}
