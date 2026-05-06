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
use hidrawd::{HidBatch, HidDeviceKind};
use key_repeat::{MonotonicNs, RepeatEngine, RepeatKey};
use keymaps::{KeyAction, KeyOutput, Keymap, LayoutId, Modifiers};
use pointer_accel::PointerAccel;
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
    pointer_x: i32,
    pointer_y: i32,
    modifiers: ModifierState,
    text_focus: bool,
    ime_visible: bool,
}

impl<R: RouteTarget> InputdService<R> {
    pub fn new(router: R, config: InputdConfig) -> Result<Self, InputdError> {
        let (width, height) = router.bounds();
        let pointer = config.initial_pointer();
        if pointer.x() < 0
            || pointer.y() < 0
            || u32::try_from(pointer.x()).ok().map_or(true, |x| x >= width)
            || u32::try_from(pointer.y()).ok().map_or(true, |y| y >= height)
        {
            return Err(InputdError::InitialPointerOutOfBounds { x: pointer.x(), y: pointer.y() });
        }

        Ok(Self {
            router,
            layout: config.layout(),
            keymap: Keymap::new(config.layout()),
            repeat: RepeatEngine::new(config.repeat()),
            pointer_accel: PointerAccel::new(config.pointer_accel()).map_err(InputdError::from)?,
            queue_capacity: config.queue_capacity().raw(),
            dispatch_log: Vec::new(),
            pointer_x: pointer.x(),
            pointer_y: pointer.y(),
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
            HidDeviceKind::Mouse => self.apply_mouse(batch.events()),
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

    fn apply_mouse(&mut self, events: &[HidEvent]) -> Result<(), InputdError> {
        let mut dx = 0;
        let mut dy = 0;
        let mut absolute_x = None;
        let mut absolute_y = None;
        let mut pointer_down = false;
        for event in events {
            match event.kind() {
                HidEventKind::Rel if event.code().raw() == RelativeAxis::X.event_code() => {
                    dx += event.value().raw();
                }
                HidEventKind::Rel if event.code().raw() == RelativeAxis::Y.event_code() => {
                    dy += event.value().raw();
                }
                HidEventKind::Abs if event.code().raw() == AbsoluteAxis::X.event_code() => {
                    absolute_x = Some(event.value().raw());
                }
                HidEventKind::Abs if event.code().raw() == AbsoluteAxis::Y.event_code() => {
                    absolute_y = Some(event.value().raw());
                }
                HidEventKind::Btn if event.code().raw() == 0x110 && event.value().raw() > 0 => {
                    pointer_down = true;
                }
                _ => {}
            }
        }

        if let (Some(next_x), Some(next_y)) = (absolute_x, absolute_y) {
            self.validate_pointer_bounds(next_x, next_y)?;
            let delivery =
                self.router.route_pointer_move(next_x, next_y).map_err(InputdError::from)?;
            self.pointer_x = next_x;
            self.pointer_y = next_y;
            let dispatch = InputDispatch::PointerMove { delivery, x: next_x, y: next_y };
            self.push_dispatch(dispatch.clone())?;
        } else if dx != 0 || dy != 0 {
            let next_x =
                self.pointer_x + self.pointer_accel.apply_axis(dx).map_err(InputdError::from)?;
            let next_y =
                self.pointer_y + self.pointer_accel.apply_axis(dy).map_err(InputdError::from)?;
            self.validate_pointer_bounds(next_x, next_y)?;
            let delivery =
                self.router.route_pointer_move(next_x, next_y).map_err(InputdError::from)?;
            self.pointer_x = next_x;
            self.pointer_y = next_y;
            let dispatch = InputDispatch::PointerMove { delivery, x: next_x, y: next_y };
            self.push_dispatch(dispatch.clone())?;
        }

        if pointer_down {
            self.validate_pointer_bounds(self.pointer_x, self.pointer_y)?;
            let delivery = self
                .router
                .route_pointer_down(self.pointer_x, self.pointer_y)
                .map_err(InputdError::from)?;
            let dispatch =
                InputDispatch::PointerDown { delivery, x: self.pointer_x, y: self.pointer_y };
            self.push_dispatch(dispatch.clone())?;
        }

        Ok(())
    }

    fn validate_pointer_bounds(&self, x: i32, y: i32) -> Result<(), InputdError> {
        let (width, height) = self.router.bounds();
        if x < 0
            || y < 0
            || u32::try_from(x).ok().map_or(true, |value| value >= width)
            || u32::try_from(y).ok().map_or(true, |value| value >= height)
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
}

fn is_modifier(usage: KeyboardUsage) -> bool {
    matches!(usage.raw(), 0xe0..=0xe7)
}
