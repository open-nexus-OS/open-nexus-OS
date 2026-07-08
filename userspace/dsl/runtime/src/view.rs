// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The mounted view: retained scene + dependency-driven damage.
//!
//! v0.1 update model: `dispatch` runs the state machine, intersects the
//! changed fields with the recorded dependencies, and — only when something
//! visible depends on them — re-emits the scene. The returned [`Damage`]
//! tells the host whether layout must re-run (`Layout`) or the existing
//! geometry stays valid (`Paint`: repaint with current boxes). Subtree-scoped
//! re-emit and arena-backed zero-alloc dispatch are recorded follow-ups.

use crate::emit::{self, Damage, Dep, EmitCtx};
use crate::interact::{self, HandlerAction, HandlerEntry};
use crate::nav::Nav;
use crate::store::Value;
use crate::{DeviceEnv, EffectHost, LocaleSource, MountError, RtError, Runtime};
use alloc::{vec, vec::Vec};
use nexus_layout_types::LayoutNode;
use nexus_theme_tokens::Tokens;

pub struct View<'p> {
    pub runtime: Runtime<'p>,
    scene: LayoutNode,
    deps: Vec<Dep>,
    /// Interactive regions: (pre-order box id, handler).
    handlers: Vec<(usize, HandlerEntry)>,
    /// Route table + history; the active page drives emission.
    pub nav: Nav,
    /// i18n key table (key index → symbol id) for locale sources.
    pub keys: Vec<u32>,
    /// Root effect-events (an `@effect` trigger that nothing dispatches) —
    /// the program's initial-load effects, derived from the dataflow at mount.
    /// Run once by [`run_initial_effects`](Self::run_initial_effects).
    initial_effects: Vec<(u32, u32)>,
    /// Guards the initial-load effects to run exactly once.
    initial_effects_fired: bool,
}

impl<'p> View<'p> {
    /// Mounts the program and emits the entry page's first scene.
    ///
    /// # Errors
    /// Mount/validation errors, or emission errors from the first frame.
    pub fn mount(
        bytes: &'p [u8],
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
    ) -> Result<Self, MountError> {
        let runtime = Runtime::mount(bytes)?;
        let reader = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(bytes)
            .map_err(MountError::Ir)?;
        let root = reader.root().map_err(MountError::Ir)?;
        let nav = Nav::mount(root).map_err(MountError::Rt)?;
        let keys: Vec<u32> = root
            .get_i18n_keys()
            .map_err(|_| MountError::Ir(nexus_dsl_ir::IrError::Malformed))?
            .iter()
            .map(|k| k.get_key())
            .collect();
        // The program's INITIAL-LOAD effects, derived from the dataflow at
        // mount (no lifecycle hook in the source — see `run_initial_effects`).
        let initial_effects =
            crate::initial::root_effect_events(root).map_err(MountError::Rt)?;
        let mut view = Self {
            runtime,
            scene: LayoutNode::Spacer(nexus_layout_types::Spacer::default()),
            deps: Vec::new(),
            handlers: Vec::new(),
            nav,
            keys,
            initial_effects,
            initial_effects_fired: false,
        };
        view.emit(tokens, device, locale).map_err(MountError::Rt)?;
        Ok(view)
    }

    /// Runs the program's INITIAL-LOAD effects — ONCE, at mount. The host calls
    /// this right after [`mount`](Self::mount).
    ///
    /// There is no `on Mount` lifecycle hook in the language (that would be a
    /// second, imperative effect-trigger model — principles.md §5). Instead the
    /// initial load falls out of the dataflow: an event that carries an
    /// `@effect` but is dispatched by NOTHING (no handler, no reducer, no other
    /// effect) is a ROOT — it can only ever run at mount, so the runtime runs
    /// it. Writing the obvious program (`@effect on Load { … }` with nothing
    /// dispatching `Load`) just loads; there is no lifecycle code to write.
    ///
    /// # Errors
    /// Runtime errors from the dispatched root events.
    pub fn run_initial_effects(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
    ) -> Result<Damage, RtError> {
        if self.initial_effects_fired {
            return Ok(Damage::None);
        }
        self.initial_effects_fired = true;
        let roots = self.initial_effects.clone();
        let mut damage = Damage::None;
        for (event, case) in roots {
            let d = self.dispatch(tokens, device, locale, host, event, case, Vec::new())?;
            if d > damage {
                damage = d;
            }
        }
        Ok(damage)
    }

    /// The retained scene (feed to `LayoutEngine`/painter).
    #[must_use]
    pub fn scene(&self) -> &LayoutNode {
        &self.scene
    }

    #[must_use]
    pub fn deps(&self) -> &[Dep] {
        &self.deps
    }

    /// Interactive regions of the current scene.
    #[must_use]
    pub fn handlers(&self) -> &[(usize, HandlerEntry)] {
        &self.handlers
    }

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
        let Some(trigger_sym) = self
            .runtime
            .symbols()
            .iter()
            .position(|s| s == trigger)
            .map(|i| i as u32)
        else {
            return Ok(None);
        };
        let Some(entry) = interact::hit(&self.handlers, boxes, trigger_sym, x, y) else {
            return Ok(None);
        };
        match entry.action.clone() {
            HandlerAction::Dispatch { event, case, payload } => {
                self.dispatch(tokens, device, locale, host, event, case, payload).map(Some)
            }
            HandlerAction::Navigate { path } => {
                self.navigate(tokens, device, locale, &path).map(Some)
            }
            HandlerAction::Bind { store, path } => {
                // Tap on a bound Bool flips it (the Toggle contract); other
                // value kinds arrive via `text_input`-style entry points.
                let current = self.runtime.read_binding(store, &path).cloned();
                let next = match current {
                    Some(Value::Bool(b)) => Value::Bool(!b),
                    _ => return Ok(None),
                };
                let changes = self.runtime.write_binding(store, &path, next)?;
                self.apply_changes(tokens, device, locale, &changes).map(Some)
            }
        }
    }

    /// Writes text into the innermost Change-bound field containing (x, y)
    /// (the host/OS text-input entry point until focus lands).
    ///
    /// # Errors
    /// Runtime errors from the write/emission.
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
        let Some(trigger_sym) = self
            .runtime
            .symbols()
            .iter()
            .position(|s| s == "Change")
            .map(|i| i as u32)
        else {
            return Ok(None);
        };
        let Some(entry) = interact::hit(&self.handlers, boxes, trigger_sym, x, y) else {
            return Ok(None);
        };
        let HandlerAction::Bind { store, path } = entry.action.clone() else {
            return Ok(None);
        };
        let changes = self.runtime.write_binding(
            store,
            &path,
            Value::Str(alloc::string::String::from(text)),
        )?;
        self.apply_changes(tokens, device, locale, &changes).map(Some)
    }

    /// Test/host helper: map externally produced changes onto the dep set
    /// and re-emit (the pointer/text paths call this internally).
    ///
    /// # Errors
    /// Emission errors.
    pub fn dispatch_noop_reemit(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        changes: &[crate::ChangedField],
    ) -> Damage {
        self.apply_changes(tokens, device, locale, changes).unwrap_or(Damage::None)
    }

    /// Maps changed fields onto the dep set (shared by dispatch + bindings).
    fn apply_changes(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        changes: &[crate::ChangedField],
    ) -> Result<Damage, RtError> {
        let mut damage = Damage::None;
        for change in changes {
            let Some(field_sym) = self
                .runtime
                .stores()
                .get(change.store as usize)
                .and_then(|s| s.field_sym(change.field as usize))
            else {
                continue;
            };
            for dep in &self.deps {
                if dep.store == change.store && dep.field == field_sym {
                    damage = damage.max(dep.damage);
                }
            }
        }
        if damage != Damage::None {
            self.emit(tokens, device, locale)?;
        }
        Ok(damage)
    }

    /// Re-emits the scene from committed state.
    fn emit(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
    ) -> Result<(), RtError> {
        let bytes_reader = self.runtime.reader();
        let root = bytes_reader.root().map_err(|_| RtError::Malformed)?;
        let components = root.get_components().map_err(|_| RtError::Malformed)?;
        let component = components.get(self.nav.current().page);
        let view_root = component.get_view().map_err(|_| RtError::Malformed)?;
        let mut locals: Vec<Option<Value>> = vec![None; 64];
        self.deps.clear();
        let symbols = self.runtime.symbols().to_vec();
        let mut handlers: Vec<HandlerEntry> = Vec::new();
        let mut ctx = EmitCtx {
            stores: self.runtime.stores(),
            locals: &mut locals,
            params: &[],
            device,
            locale,
            tokens,
            symbols: &symbols,
            deps: &mut self.deps,
            handlers: &mut handlers,
            path: Vec::new(),
            components,
        };
        self.scene = emit::emit_view(&mut ctx, view_root)?;
        // Resolve handler paths to pre-order box ids against the new scene.
        self.handlers.clear();
        for entry in handlers {
            if let Some(box_id) = interact::path_to_box_id(&self.scene, &entry.path) {
                self.handlers.push((box_id, entry));
            }
        }
        Ok(())
    }

    /// Navigates to a route path: pushes onto the bounded history and
    /// re-emits the new page. Route params become the page's param slice
    /// with the param-binding wave (TASK-0077 remainder).
    ///
    /// # Errors
    /// Unmatched path, full history, or emission errors.
    pub fn navigate(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        path: &str,
    ) -> Result<Damage, RtError> {
        self.nav.push(path)?;
        self.emit(tokens, device, locale)?;
        Ok(Damage::Layout)
    }

    /// Pops the route history (root always remains). Re-emits on change.
    ///
    /// # Errors
    /// Emission errors.
    pub fn navigate_back(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
    ) -> Result<Damage, RtError> {
        if self.nav.back() {
            self.emit(tokens, device, locale)?;
            Ok(Damage::Layout)
        } else {
            Ok(Damage::None)
        }
    }

    /// The underlying runtime (event/name lookups for hosts and tools).
    #[must_use]
    pub fn runtime(&self) -> &crate::Runtime<'p> {
        &self.runtime
    }

    /// Dispatches an event; re-emits only when a visible dependency changed.
    ///
    /// # Errors
    /// Runtime errors from reduce/effects/emission.
    pub fn dispatch(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        event: u32,
        case: u32,
        payload: Vec<Value>,
    ) -> Result<Damage, RtError> {
        let changes = self.runtime.dispatch(device, locale, host, event, case, payload)?;
        self.apply_changes(tokens, device, locale, &changes)
    }
}
