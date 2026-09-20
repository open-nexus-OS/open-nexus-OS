// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: where an interaction GOES — the `View`'s routing surface, split out
//! of `view.rs` under the structure ratchet (TASK-0077B P2b).
//!
//! `interact.rs` answers "what did the pointer land on"; this answers "and then
//! what happens": the hit handler's action is dispatched, navigated, or — for a
//! two-way bind — turned into a value by the rule the handler carries
//! (`crate::bind`) and committed through the one store mutation path. Hover is
//! here too because it is the same hit-test without the action: presentation
//! only, no dispatch, no re-layout.
//!
//! `view.rs` keeps mounting, emission, dispatch and navigation.
//!
//! OWNERS: @ui
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: `dsl_goldens::scenes`, `dsl_apps_conformance::shell_control_center`

use super::{Damage, RtError, ScrollView, View};
use crate::bind;
use crate::interact::{self, HandlerAction};
use crate::{DeviceEnv, EffectHost, LocaleSource};
use nexus_theme_tokens::Tokens;

impl View<'_> {
    /// Routes a pointer event: finds the innermost handler for `trigger`
    /// (an interned symbol name, e.g. "Tap") containing the point and
    /// dispatches its captured target. Returns the damage, or `None` if
    /// nothing was hit.
    ///
    /// # Errors
    /// Runtime errors from the dispatch.
    #[allow(clippy::too_many_arguments)]
    pub fn pointer(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        boxes: &[nexus_layout::LayoutBox],
        trigger: &str,
        x: nexus_layout_types::FxPx,
        y: nexus_layout_types::FxPx,
    ) -> Result<Option<Damage>, RtError> {
        self.pointer_scrolled(tokens, device, locale, host, boxes, trigger, x, y, None)
    }

    /// [`Self::pointer`] under the paint-time scroll transform (`scroll` =
    /// viewport rect + offsets, see `interact::hit_scrolled`).
    ///
    /// # Errors
    /// Runtime errors from the dispatch.
    #[allow(clippy::too_many_arguments)]
    pub fn pointer_scrolled(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        boxes: &[nexus_layout::LayoutBox],
        trigger: &str,
        x: nexus_layout_types::FxPx,
        y: nexus_layout_types::FxPx,
        scroll: Option<ScrollView>,
    ) -> Result<Option<Damage>, RtError> {
        let Some(trigger_sym) =
            self.runtime.symbols().iter().position(|s| s == trigger).map(|i| i as u32)
        else {
            return Ok(None);
        };
        let Some(hit) = interact::hit_scrolled(&self.handlers, boxes, trigger_sym, x, y, scroll)
        else {
            return Ok(None);
        };
        // The instance the hit handler belongs to (TASK-0077B P1): a tap inside
        // a keyed collection item acts on THAT item's state.
        let instance = hit.entry.instance;
        match hit.entry.action.clone() {
            HandlerAction::Dispatch { event, case, payload } => self
                .dispatch_in(instance, tokens, device, locale, host, event, case, payload)
                .map(Some),
            HandlerAction::Navigate { path } => {
                self.navigate(tokens, device, locale, &path).map(Some)
            }
            // The handler carries its own derivation (IR v1.8): a Toggle's flip
            // and a Slider's fraction are the same rule applied to different
            // data, not two arms here.
            HandlerAction::Bind { store, path, value } => self.write_bound(
                tokens,
                device,
                locale,
                store,
                instance,
                &path,
                value,
                &bind::Interaction::Point { rect: hit.rect, x: hit.x, y: hit.y },
            ),
        }
    }

    /// Applies a bind rule and commits the value it produces.
    ///
    /// A rule that cannot produce a value for this interaction writes NOTHING
    /// and reports no damage — never a default, never the current value
    /// re-written (`bind::next_value`).
    ///
    /// # Errors
    /// Runtime errors from the write/re-emission.
    #[allow(clippy::too_many_arguments)]
    fn write_bound(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        store: u32,
        instance: u64,
        path: &[u32],
        value: nexus_dsl_ir::ui_ir_capnp::BindValue,
        at: &bind::Interaction,
    ) -> Result<Option<Damage>, RtError> {
        let current = self.runtime.read_binding(store, instance, path).cloned();
        let Some(next) = bind::next_value(value, current.as_ref(), at) else {
            return Ok(None);
        };
        let changes = self.runtime.write_binding(store, instance, path, next)?;
        self.apply_changes(tokens, device, locale, &changes).map(Some)
    }

    /// Presentation-only hit-test: the pre-order box id (`LayoutBox::node_id`)
    /// of the innermost `trigger` handler under (x, y), without running its
    /// action. This is the HOVER anchor — the host tracks it and blends the
    /// interaction wash at paint time (no store dispatch, no re-layout).
    #[must_use]
    pub fn hover_box_id(
        &self,
        boxes: &[nexus_layout::LayoutBox],
        trigger: &str,
        x: nexus_layout_types::FxPx,
        y: nexus_layout_types::FxPx,
    ) -> Option<usize> {
        self.hover_box_id_scrolled(boxes, trigger, x, y, None)
    }

    /// Dispatches the FIRST handler bound to `trigger` WITHOUT a hit-test —
    /// for container-scoped events that are not pointer positions (e.g.
    /// `EndReached` when the scroll offset nears the content end: the
    /// viewport fired it, no pixel was "hit"). Returns the damage, `None`
    /// when the page declares no such handler.
    ///
    /// # Errors
    /// Runtime errors from the dispatch.
    pub fn fire_trigger(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        trigger: &str,
    ) -> Result<Option<Damage>, RtError> {
        let Some(trigger_sym) =
            self.runtime.symbols().iter().position(|s| s == trigger).map(|i| i as u32)
        else {
            return Ok(None);
        };
        let Some(entry) = self
            .handlers
            .iter()
            .find(|(_, e)| e.trigger == trigger_sym)
            .map(|(_, e)| e.action.clone())
        else {
            return Ok(None);
        };
        match entry {
            HandlerAction::Dispatch { event, case, payload } => {
                self.dispatch(tokens, device, locale, host, event, case, payload).map(Some)
            }
            HandlerAction::Navigate { path } => {
                self.navigate(tokens, device, locale, &path).map(Some)
            }
            HandlerAction::Bind { .. } => Ok(None),
        }
    }

    /// [`Self::hover_box_id`] under the paint-time scroll transform.
    #[must_use]
    pub fn hover_box_id_scrolled(
        &self,
        boxes: &[nexus_layout::LayoutBox],
        trigger: &str,
        x: nexus_layout_types::FxPx,
        y: nexus_layout_types::FxPx,
        scroll: Option<ScrollView>,
    ) -> Option<usize> {
        let trigger_sym =
            self.runtime.symbols().iter().position(|s| s == trigger).map(|i| i as u32)?;
        interact::hit_scrolled(&self.handlers, boxes, trigger_sym, x, y, scroll).map(|h| h.box_id)
    }

    /// Writes text into the innermost Change-bound field containing (x, y)
    /// (the host/OS text-input entry point until focus lands).
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
    // reason: host text-input entry point — args are the ambient environments
    // (tokens/device/locale), the layout, the point and the text payload.
    #[allow(clippy::too_many_arguments)]
    pub fn text_input(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        boxes: &[nexus_layout::LayoutBox],
        x: nexus_layout_types::FxPx,
        y: nexus_layout_types::FxPx,
        text: &str,
    ) -> Result<Option<Damage>, RtError> {
        let Some(trigger_sym) =
            self.runtime.symbols().iter().position(|s| s == "Change").map(|i| i as u32)
        else {
            return Ok(None);
        };
        let Some(hit) = interact::hit(&self.handlers, boxes, trigger_sym, x, y) else {
            return Ok(None);
        };
        let HandlerAction::Bind { store, path, value } = hit.entry.action.clone() else {
            return Ok(None);
        };
        let instance = hit.entry.instance;
        self.write_bound(
            tokens,
            device,
            locale,
            store,
            instance,
            &path,
            value,
            &bind::Interaction::Text(text),
        )
    }
}
