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
use crate::route::RouteTarget;
use crate::{ImeHook, InputDispatch, InputdError};
use alloc::vec::Vec;
use hid::{AbsoluteAxis, HidEvent, HidEventKind, KeyboardUsage, RelativeAxis};
use hidrawd::{HidBatch, HidDeviceKind, PointerSource};
use key_repeat::{MonotonicNs, RepeatEngine, RepeatKey};
use keymaps::{KeyAction, KeyOutput, Keymap, LayoutId, Modifiers};
use pointer_accel::PointerAccel;
use pointer_state::{PointerPosition, PointerSpace, PointerState, PointerTransform};
use touch::{TouchEvent, TouchPhase};

#[derive(Debug, Clone, Copy, Default)]
struct ModifierState {
    shift: bool,
    control: bool,
    alt_gr: bool,
}

impl ModifierState {
    fn apply_key(&mut self, usage: KeyboardUsage, pressed: bool) {
        match usage.raw() {
            0xe0 | 0xe4 => self.control = pressed,
            0xe1 | 0xe5 => self.shift = pressed,
            0xe6 => self.alt_gr = pressed,
            _ => {}
        }
    }

    #[must_use]
    fn snapshot(self) -> Modifiers {
        let mut modifiers = Modifiers::default();
        if self.shift {
            modifiers = modifiers.with_shift();
        }
        if self.control {
            modifiers = modifiers.with_control();
        }
        if self.alt_gr {
            modifiers = modifiers.with_alt_gr();
        }
        modifiers
    }
}

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
    primary_pointer_held: bool,
    held_non_modifier_keys: [bool; 256],
    held_non_modifier_key_count: usize,
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
            primary_pointer_held: false,
            held_non_modifier_keys: [false; 256],
            held_non_modifier_key_count: 0,
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
            if pressed {
                let output = self
                    .keymap
                    .resolve(usage, self.modifiers.snapshot())
                    .map_err(InputdError::from)?;
                let delivery = self
                    .router
                    .route_keyboard(u32::from(event.code().raw()))
                    .map_err(InputdError::from)?;
                let repeat_key = RepeatKey::new(event.code().raw()).map_err(InputdError::from)?;
                self.repeat
                    .press(repeat_key, MonotonicNs::new(event.timestamp().raw()))
                    .map_err(InputdError::from)?;
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

        if absolute_x.is_some() || absolute_y.is_some() {
            let display = self.pointer_state.apply_absolute(absolute_x, absolute_y);
            let route = self.pointer_transform.display_to_route(display);
            let delivery = self
                .router
                .try_coalesce_pointer_move(route.x, route.y)
                .map_err(InputdError::from)?;
            let dispatch = InputDispatch::PointerMove { delivery, x: route.x, y: route.y };
            self.push_dispatch(dispatch.clone())?;
            self.active_pointer_source = pointer_source.or(Some(PointerSource::TabletAbsolute));
        } else if dx != 0 || dy != 0 {
            if pointer_source == Some(PointerSource::MouseRelative)
                && self.relative_motion_blocked_by_absolute_source()
            {
                return self.finish_pointer_side_effects(pointer_down, wheel_delta);
            }
            let display = self.pointer_state.apply_relative(
                self.pointer_accel.apply_axis(dx).map_err(InputdError::from)?,
                self.pointer_accel.apply_axis(dy).map_err(InputdError::from)?,
            );
            let route = self.pointer_transform.display_to_route(display);
            let delivery = self
                .router
                .try_coalesce_pointer_move(route.x, route.y)
                .map_err(InputdError::from)?;
            let dispatch = InputDispatch::PointerMove { delivery, x: route.x, y: route.y };
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
            let route = self.pointer_state.route_position(self.pointer_transform);
            self.validate_pointer_bounds(route.x, route.y)?;
            let delivery =
                self.router.route_pointer_down(route.x, route.y).map_err(InputdError::from)?;
            let dispatch = InputDispatch::PointerDown { delivery, x: route.x, y: route.y };
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

fn is_modifier(usage: KeyboardUsage) -> bool {
    matches!(usage.raw(), 0xe0..=0xe7)
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
