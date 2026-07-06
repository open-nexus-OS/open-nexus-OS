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
use crate::interact::{self, HandlerEntry};
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
    entry: u32,
    /// i18n key table (key index → symbol id) for locale sources.
    pub keys: Vec<u32>,
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
        let entry = root.get_entry_page();
        let keys: Vec<u32> = root
            .get_i18n_keys()
            .map_err(|_| MountError::Ir(nexus_dsl_ir::IrError::Malformed))?
            .iter()
            .map(|k| k.get_key())
            .collect();
        let mut view = Self {
            runtime,
            scene: LayoutNode::Spacer(nexus_layout_types::Spacer::default()),
            deps: Vec::new(),
            handlers: Vec::new(),
            entry,
            keys,
        };
        view.emit(tokens, device, locale).map_err(MountError::Rt)?;
        Ok(view)
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
        let (event, case, payload) = (entry.event, entry.case, entry.payload.clone());
        self.dispatch(tokens, device, locale, host, event, case, payload).map(Some)
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
        let component = components.get(self.entry);
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
        let mut damage = Damage::None;
        for change in &changes {
            // Deps store field SYMBOLS; changes carry field indices.
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
}
