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

mod route;

use crate::anim::AnimIntent;
use crate::emit::{self, Damage, Dep, EmitCtx};
use crate::interact::{self, HandlerEntry, ScrollView};
use crate::nav::Nav;
use crate::store::Value;
use crate::{DeviceEnv, EffectHost, LocaleSource, MountError, RtError, Runtime};
use alloc::{vec, vec::Vec};
use nexus_layout_types::LayoutNode;
use nexus_theme_tokens::Tokens;

/// A scope the host wants held for the duration of EMISSION (TASK-0077C P2b).
///
/// The runtime does not know what this is for and must not: from here it is
/// "the host would like to be told when a scene is being built". The app-host
/// routes emission into a generation arena whose memory is recycled two frames
/// later (ADR-0065); the host test harness routes it into a POISONING arena to
/// prove nothing reads recycled memory; every other host and test uses nothing.
///
/// Set ONCE after mount, held by the one funnel every emitting entry point
/// reaches (`View::emit`). A scope argument threaded through six entry points
/// would be an alternate surface for a fact that never varies per call
/// (`principles.md` §5).
///
/// **The contract that makes it safe — and that the harness enforces:** while
/// the scope is held, emission touches NOTHING that outlives the frame. Store
/// mutations, focus revalidation and the live-instance sweep run after the
/// guard has dropped, and every retained output (`deps`, `handlers`,
/// `animations`, the scene) is a FRESH value each frame — a buffer kept across
/// frames with `clear()` would be written in generation g+1 and reset
/// underneath its owner in g+2.
pub trait FrameScope {
    /// Called before emission begins; the returned guard is dropped after.
    fn enter(&self) -> FrameScopeGuard;
}

/// What a [`FrameScope`] hands back. Dropping it ends the scope.
pub struct FrameScopeGuard {
    on_exit: Option<fn()>,
}

impl FrameScopeGuard {
    /// A guard that runs `on_exit` when emission finishes.
    #[must_use]
    pub fn new(on_exit: fn()) -> Self {
        Self { on_exit: Some(on_exit) }
    }

    /// A guard that does nothing.
    #[must_use]
    pub fn inert() -> Self {
        Self { on_exit: None }
    }
}

impl Drop for FrameScopeGuard {
    fn drop(&mut self) {
        if let Some(on_exit) = self.on_exit.take() {
            on_exit();
        }
    }
}

pub struct View<'p> {
    pub runtime: Runtime<'p>,
    pub(crate) scene: LayoutNode,
    deps: Vec<Dep>,
    /// Interactive regions: (pre-order box id, handler).
    pub(crate) handlers: Vec<(usize, HandlerEntry)>,
    /// The focused text field (tap-to-focus; None = no field focused).
    pub(crate) focused_text: Option<crate::focus::FocusedText>,
    /// The scene's kinded overlays (TASK-0074): rebuilt by every emit, read by
    /// the routing (`modal_confine`) and the host (depth, transient timeouts).
    pub(crate) overlays: crate::overlay::OverlayStack,
    /// The reason of the last `on Dismiss` the runtime fired, until the host
    /// takes it for its marker (`take_dismissed`).
    pub(crate) last_dismiss: Option<crate::overlay::DismissReason>,
    /// Motion intents of the current scene: (pre-order box id, intent). The
    /// host reads these each frame, seeds its `AnimationDriver` on a change,
    /// and paints the interpolated result (docs/dev/ui/foundations/animation.md).
    animations: Vec<(usize, AnimIntent)>,
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
    /// The host's emission scope (TASK-0077C P2b), set once after mount.
    frame_scope: Option<&'static dyn FrameScope>,
    /// Platform SAFE AREA: surface rows at the top that belong to the shell
    /// status bar. Re-applied to the scene root on every emit, so a re-emit
    /// (navigate, theme swap, size-class re-select) can never lose it.
    safe_area_top: nexus_layout_types::FxPx,
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
        let initial_effects = crate::initial::root_effect_events(root).map_err(MountError::Rt)?;
        let mut view = Self {
            runtime,
            scene: LayoutNode::Spacer(nexus_layout_types::Spacer::default()),
            deps: Vec::new(),
            handlers: Vec::new(),
            focused_text: None,
            overlays: crate::overlay::OverlayStack::new(),
            last_dismiss: None,
            animations: Vec::new(),
            nav,
            keys,
            initial_effects,
            initial_effects_fired: false,
            frame_scope: None,
            safe_area_top: nexus_layout_types::FxPx::ZERO,
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

    /// Reserve `top` surface rows at the top of the page for shell chrome.
    ///
    /// The compositor owns this number — it alone knows the status-bar height
    /// AND whether this particular window is maximized. Applying it here means
    /// every app gets it for free and no page restates the bar's height; the
    /// one app that used to hard-code a spacer (`settings`, at 40px against a
    /// 36px bar) drops it.
    ///
    /// Returns whether it could be applied — a leaf page root cannot be
    /// padded, and the caller should say so rather than render under the bar.
    pub fn set_safe_area_top(&mut self, top: nexus_layout_types::FxPx) -> bool {
        let delta = top - self.safe_area_top;
        self.safe_area_top = top;
        if delta == nexus_layout_types::FxPx::ZERO {
            return true;
        }
        // Adjust the LIVE scene too: the caller relayouts right after, and a
        // re-emit is not guaranteed to happen in between.
        self.scene.inset_top(delta)
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

    /// Installs the host's emission scope (TASK-0077C P2b). Called once after
    /// mount; a `View` without one emits onto the ordinary heap.
    pub fn set_frame_scope(&mut self, scope: &'static dyn FrameScope) {
        self.frame_scope = Some(scope);
    }

    /// Interactive regions of the current scene.
    #[must_use]
    pub fn handlers(&self) -> &[(usize, HandlerEntry)] {
        &self.handlers
    }

    /// The scene's kinded overlays (TASK-0074): modal depth, the topmost
    /// modal, the transient layers and their declared timeouts.
    #[must_use]
    pub fn overlays(&self) -> &crate::overlay::OverlayStack {
        &self.overlays
    }

    /// The path every pointer/focus hit must lie under while a modal is open:
    /// the topmost modal's subtree. `None` = nothing is confined.
    #[must_use]
    pub fn modal_confine(&self) -> Option<&[u32]> {
        self.overlays.top_modal().map(|e| e.path.as_slice())
    }

    /// The reason of the last runtime-fired `on Dismiss`, once (host marker).
    pub fn take_dismissed(&mut self) -> Option<crate::overlay::DismissReason> {
        self.last_dismiss.take()
    }

    /// Motion intents of the current scene (`.animate`/`.transition`/
    /// `.effect`), resolved to pre-order box ids. The host diffs these across
    /// re-emits to drive the animation engine; empty when the page declares
    /// no motion.
    #[must_use]
    pub fn animations(&self) -> &[(usize, AnimIntent)] {
        &self.animations
    }
    /// Test/host helper: map externally produced changes onto the dep set
    /// and re-emit (the pointer/text paths call this internally).
    ///
    /// # Errors
    /// Emission errors.
    /// Forces a re-emit under the CURRENT state — for device-ENVIRONMENT
    /// changes (a resize crossing a `device.sizeClass` breakpoint, an
    /// orientation flip) that alter `if device.*` arms without any store
    /// change. Stores survive (no remount); deps/handlers/intents refresh.
    ///
    /// # Errors
    /// Emission errors.
    pub fn reemit(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
    ) -> Result<(), RtError> {
        self.emit(tokens, device, locale)
    }

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
    pub(crate) fn apply_changes(
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
                .and_then(|s| s.root().field_sym(change.field as usize))
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
    ///
    /// Two halves with a hard line between them (TASK-0077C P2b, ADR-0065):
    ///
    /// 1. **The frame**, inside the host's [`FrameScope`]: build the scene and
    ///    resolve handlers and motion intents against it. Everything allocated
    ///    here may live in a generation arena and be recycled two frames later,
    ///    so every output is a FRESH value — never a buffer kept from the last
    ///    frame with `clear()`. A kept buffer is written in generation g+1 and
    ///    reset underneath its owner in g+2; the poisoning harness in
    ///    `dsl_apps_conformance::arena_invariant` aborts on exactly that.
    /// 2. **After the scope**: everything that OUTLIVES the frame — the
    ///    live-instance sweep over the stores, and the text-focus record with
    ///    its cloned path and payload — allocates on the ordinary heap. (The
    ///    focus record is rebuilt on every emit, so it would in fact never
    ///    reach two generations old; it is kept out on principle, so that a
    ///    future holder of it is not the one to discover the arena.)
    fn emit(
        &mut self,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
    ) -> Result<(), RtError> {
        let (scene, deps, handlers, animations, live_instances, overlays) = {
            let _scope = self.frame_scope.map(FrameScope::enter);
            let bytes_reader = self.runtime.reader();
            let root = bytes_reader.root().map_err(|_| RtError::Malformed)?;
            let components = root.get_components().map_err(|_| RtError::Malformed)?;
            let component = components.get(self.nav.current().page);
            let view_root = component.get_view().map_err(|_| RtError::Malformed)?;
            let mut locals: Vec<Option<Value>> = vec![None; 64];
            let mut live_instances: Vec<u64> = Vec::new();
            let symbols = self.runtime.symbols().to_vec();
            // Fresh per frame — see the contract above.
            let mut deps: Vec<Dep> = Vec::new();
            let mut raw_handlers: Vec<HandlerEntry> = Vec::new();
            let mut anim_intents: Vec<(Vec<u32>, AnimIntent)> = Vec::new();
            let mut overlays = crate::overlay::OverlayStack::new();
            let mut ctx = EmitCtx {
                stores: self.runtime.stores(),
                // A component body starts at the root instance; a keyed
                // collection inside it re-bases per item (TASK-0077B P1).
                instance: crate::store::ROOT_INSTANCE,
                live_instances: &mut live_instances,
                locals: &mut locals,
                params: &[],
                device,
                locale,
                tokens,
                symbols: &symbols,
                deps: &mut deps,
                handlers: &mut raw_handlers,
                anim_intents: &mut anim_intents,
                overlays: &mut overlays,
                path: Vec::new(),
                components,
                // The entry page has no caller, so no slot frame.
                slots: None,
            };
            let mut scene = emit::emit_view(&mut ctx, view_root)?;
            drop(ctx);
            // The status-bar rows the compositor reserved. Applied on every
            // emit, because the scene is rebuilt from scratch each time.
            if self.safe_area_top > nexus_layout_types::FxPx::ZERO {
                let _ = scene.inset_top(self.safe_area_top);
            }
            // Resolve handler and motion-intent paths to pre-order box ids
            // against the NEW scene (one box id per animated node, so the host
            // can key its `AnimationDriver` by `node_id`).
            let mut handlers: Vec<(usize, HandlerEntry)> = Vec::with_capacity(raw_handlers.len());
            for entry in raw_handlers {
                if let Some(box_id) = interact::path_to_box_id(&scene, &entry.path) {
                    handlers.push((box_id, entry));
                }
            }
            let mut animations: Vec<(usize, AnimIntent)> = Vec::with_capacity(anim_intents.len());
            for (path, intent) in anim_intents {
                if let Some(box_id) = interact::path_to_box_id(&scene, &path) {
                    animations.push((box_id, intent));
                }
            }
            // The overlay layers' own box ids (the backdrop test needs the
            // layer's rect; `0` = the node did not resolve, which never
            // happens for an emitted container).
            for entry in overlays.entries_mut() {
                entry.box_id = interact::path_to_box_id(&scene, &entry.path).unwrap_or(0);
            }
            (scene, deps, handlers, animations, live_instances, overlays)
        };
        self.scene = scene;
        self.deps = deps;
        self.handlers = handlers;
        self.animations = animations;
        self.overlays = overlays;
        // Per-instance state is dropped for instances this emit did NOT
        // produce (TASK-0077B P1): a row that left the collection takes its
        // fields with it, so storage tracks the live set instead of growing
        // with every key ever seen. Nothing is dropped when no keyed
        // collection emitted, because then every keyed store is still at its
        // root instance. This touches the STORES, so it runs after the scope.
        let live: alloc::collections::BTreeSet<u64> = live_instances
            .iter()
            .copied()
            .chain(core::iter::once(crate::store::ROOT_INSTANCE))
            .collect();
        for slot in self.runtime.stores_mut() {
            slot.retain_instances(&live);
        }
        // The focus record outlives many frames and clones a path and a
        // payload out of the new handlers — on the ordinary heap, here.
        self.revalidate_text_focus();
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
    // reason: event dispatch entry point — args are the ambient environments
    // (tokens/device/locale/host) plus the event id, case and payload.
    #[allow(clippy::too_many_arguments)]
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
        self.dispatch_in(
            crate::store::ROOT_INSTANCE,
            tokens,
            device,
            locale,
            host,
            event,
            case,
            payload,
        )
    }

    /// Dispatch into a specific INSTANCE (TASK-0077B P1) — what an interaction
    /// inside a keyed collection item uses so the reducer writes that item's
    /// own state.
    ///
    /// # Errors
    /// As [`Self::dispatch`].
    #[allow(clippy::too_many_arguments)]
    pub fn dispatch_in(
        &mut self,
        instance: u64,
        tokens: &dyn Tokens,
        device: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        event: u32,
        case: u32,
        payload: Vec<Value>,
    ) -> Result<Damage, RtError> {
        let changes =
            self.runtime.dispatch_in(instance, device, locale, host, event, case, payload)?;
        self.apply_changes(tokens, device, locale, &changes)
    }
}
