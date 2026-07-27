// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! View emission: IR view tree + committed state → `LayoutNode` scene,
//! recording state→node dependencies with their invalidation class.

mod modifiers;
mod slots;
use modifiers::apply_modifier;
use slots::{emit_slot_items, SlotFrame};

use crate::anim::{AnimIntent, AnimKind};
use crate::interact::{HandlerAction, HandlerEntry};
use crate::reduce::{eval, EvalCtx};
use crate::registry::{self, Mods};
use crate::store::{StoreState, Value};
use crate::{DeviceEnv, LocaleSource, RtError};
use alloc::{string::String, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;
use nexus_layout_types::LayoutNode;
use nexus_theme_tokens::Tokens;

/// What a state change to a depended-on field must invalidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Damage {
    None,
    Paint,
    Layout,
}

/// One recorded dependency: this (store, field) feeds a site of `damage` class.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dep {
    pub store: u32,
    pub field: u32,
    pub damage: Damage,
}

/// `'a` = the current emit frame (borrowed state + the slot-frame chain);
/// `'p` = the program message the capnp readers point into. Two lifetimes so
/// a `SlotFrame` can hold readers from the message while borrowing the frame
/// that created it — without depending on capnp reader variance.
pub(crate) struct EmitCtx<'a, 'p> {
    pub stores: &'a [StoreState],
    pub locals: &'a mut [Option<Value>],
    pub params: &'a [Value],
    pub device: &'a dyn DeviceEnv,
    pub locale: &'a dyn LocaleSource,
    pub tokens: &'a dyn Tokens,
    pub symbols: &'a [String],
    pub deps: &'a mut Vec<Dep>,
    /// Interactive regions collected during emission (paths in the final tree).
    pub handlers: &'a mut Vec<HandlerEntry>,
    /// Animation intents collected during emission — `(node path, intent)`,
    /// resolved to a pre-order box id after emit (same walk as `handlers`).
    pub anim_intents: &'a mut Vec<(Vec<u32>, AnimIntent)>,
    /// Absolute child-index path of the node currently being emitted.
    pub path: Vec<u32>,
    pub components: capnp::struct_list::Reader<'p, ir::component::Owned>,
    /// The slot bodies THIS component instance received (RFC-0084), or `None`
    /// for a page / a component emitted outside any reference.
    pub slots: Option<&'a SlotFrame<'a, 'p>>,
}

impl EmitCtx<'_, '_> {
    fn eval(&mut self, expr: ir::expr::Reader<'_>) -> Result<Value, RtError> {
        let mut ctx = EvalCtx {
            stores: self.stores,
            locals: self.locals,
            params: self.params,
            device: self.device,
            locale: self.locale,
        };
        eval(&mut ctx, expr)
    }

    fn symbol(&self, id: u32) -> &str {
        self.symbols.get(id as usize).map(String::as_str).unwrap_or("")
    }

    /// Records every state read inside `expr` as a dependency of `damage`.
    fn record_deps(&mut self, expr: ir::expr::Reader<'_>, damage: Damage) {
        use ir::expr::Which;
        match expr.which() {
            Ok(Which::FieldGet(Ok(get))) => {
                if let (store, Ok(path)) = (get.get_store(), get.get_path()) {
                    if !path.is_empty() {
                        // Field symbol → index resolution happens at damage
                        // time; store the *symbol* so deps survive re-emits.
                        self.deps.push(Dep { store, field: path.get(0), damage });
                    }
                }
            }
            Ok(Which::LitList(Ok(items))) => {
                for item in items.iter() {
                    self.record_deps(item, damage);
                }
            }
            Ok(Which::LitEnum(Ok(lit))) => {
                if let Ok(payload) = lit.get_payload() {
                    for item in payload.iter() {
                        self.record_deps(item, damage);
                    }
                }
            }
            Ok(Which::UnOp(Ok(un))) => {
                if let Ok(operand) = un.get_operand() {
                    self.record_deps(operand, damage);
                }
            }
            Ok(Which::BinOp(Ok(bin))) => {
                if let Ok(lhs) = bin.get_lhs() {
                    self.record_deps(lhs, damage);
                }
                if let Ok(rhs) = bin.get_rhs() {
                    self.record_deps(rhs, damage);
                }
            }
            Ok(Which::RecordGet(Ok(get))) => {
                if let Ok(base) = get.get_base() {
                    self.record_deps(base, damage);
                }
            }
            Ok(Which::FmtI18n(Ok(fmt))) => {
                if let Ok(args) = fmt.get_args() {
                    for arg in args.iter() {
                        self.record_deps(arg, damage);
                    }
                }
            }
            Ok(Which::OptionSome(Ok(inner))) => self.record_deps(inner, damage),
            _ => {}
        }
    }
}

/// Emits one view node into a `LayoutNode`.
pub(crate) fn emit_view<'p>(
    ctx: &mut EmitCtx<'_, 'p>,
    node: ir::view_node::Reader<'p>,
) -> Result<LayoutNode, RtError> {
    use ir::view_node::Which;
    match node.which().map_err(|_| RtError::Malformed)? {
        Which::Widget(widget) => {
            let widget = widget.map_err(|_| RtError::Malformed)?;
            emit_widget(ctx, widget)
        }
        Which::Branch(branch) => {
            let branch = branch.map_err(|_| RtError::Malformed)?;
            // Branch structure depends on its conds: layout class.
            let mut chosen: Option<capnp::struct_list::Reader<'_, ir::view_node::Owned>> = None;
            for arm in branch.get_arms().map_err(|_| RtError::Malformed)?.iter() {
                let cond = arm.get_cond().map_err(|_| RtError::Malformed)?;
                ctx.record_deps(cond, Damage::Layout);
                if chosen.is_none() {
                    if let Value::Bool(true) = ctx.eval(cond)? {
                        chosen = Some(arm.get_body().map_err(|_| RtError::Malformed)?);
                    }
                }
            }
            let body = match chosen {
                Some(body) => body,
                None => branch.get_else_body().map_err(|_| RtError::Malformed)?,
            };
            // A single-node body is TRULY transparent: emit it directly, no
            // wrapper box and no extra path segment. A wrapper here reset the
            // flex context — `.grow()/.height()` on the page column stopped
            // stretching once the platform-override `if` wrapped it (the
            // collapsed-shell bug). Multi-node bodies keep the column wrapper
            // so the branch stays a single node.
            if body.len() == 1 {
                return emit_view(ctx, body.get(0));
            }
            let mut children = Vec::with_capacity(body.len() as usize);
            for (j, child) in body.iter().enumerate() {
                ctx.path.push(j as u32);
                let emitted = emit_view(ctx, child)?;
                ctx.path.pop();
                children.push(emitted);
            }
            Ok(registry::build_widget("Stack", &[], &Mods::default(), ctx.tokens, children))
        }
        Which::ForEach(for_each) => {
            // Standalone collection (no widget parent): a plain column wraps
            // the items so the branch output stays one node. INSIDE a widget
            // the items SPLICE into the parent instead (emit_widget) — a
            // wrapper box there reset the parent's row/wrap flex context.
            let for_each = for_each.map_err(|_| RtError::Malformed)?;
            let mut children = Vec::new();
            emit_for_each_items(ctx, for_each, &[], 0, &mut children)?;
            Ok(registry::build_widget("Stack", &[], &Mods::default(), ctx.tokens, children))
        }
        Which::ComponentRef(component_ref) => {
            let component_ref = component_ref.map_err(|_| RtError::Malformed)?;
            let component = ctx.components.get(component_ref.get_component());
            // Resolve args in the component's prop order.
            let prop_defs = component.get_props().map_err(|_| RtError::Malformed)?;
            let args = component_ref.get_args().map_err(|_| RtError::Malformed)?;
            let mut params = Vec::with_capacity(prop_defs.len() as usize);
            for prop in prop_defs.iter() {
                let mut value = Value::Unit;
                for arg in args.iter() {
                    if arg.get_name() == prop.get_name() {
                        let expr = arg.get_value().map_err(|_| RtError::Malformed)?;
                        ctx.record_deps(expr, Damage::Layout);
                        value = ctx.eval(expr)?;
                        break;
                    }
                }
                params.push(value);
            }
            // The frame the callee's `Slot` placeholders will read. It carries
            // the CALLER's params and a snapshot of the CALLER's locals, so a
            // slot body evaluates in the scope it was WRITTEN in — not the one
            // it is rendered in (RFC-0084 §4). The snapshot is not paranoia:
            // `locals` are shared across this boundary, so a `for` inside the
            // callee can overwrite a caller loop binding before the body runs.
            let frame = SlotFrame {
                args: component_ref.get_slots().map_err(|_| RtError::Malformed)?,
                params: ctx.params,
                locals: ctx.locals.to_vec(),
            };
            // Emit the component body with its own params (locals shared —
            // slots are per-body and component bodies allocate fresh ones).
            let view = component.get_view().map_err(|_| RtError::Malformed)?;
            let path = ctx.path.clone();
            let mut inner = EmitCtx {
                stores: ctx.stores,
                locals: ctx.locals,
                params: &params,
                device: ctx.device,
                locale: ctx.locale,
                tokens: ctx.tokens,
                symbols: ctx.symbols,
                deps: ctx.deps,
                handlers: ctx.handlers,
                anim_intents: ctx.anim_intents,
                path,
                components: ctx.components,
                slots: Some(&frame),
            };
            emit_view(&mut inner, view)
        }
        Which::Slot(slot_ref) => {
            // A placeholder that is NOT a direct widget child (a lone branch-arm
            // body, say). The branch precedent applies: one node returns
            // transparently, zero or many wrap — a wrapper here cannot reset a
            // parent's flex context because there is no widget parent to reset.
            // Authors should prefer the widget-child form, which splices.
            let slot_ref = slot_ref.map_err(|_| RtError::Malformed)?;
            let mut nodes = Vec::new();
            emit_slot_items(ctx, slot_ref.get_slot(), &[], 0, &mut nodes)?;
            if nodes.len() == 1 {
                return Ok(nodes.remove(0));
            }
            Ok(registry::build_widget("Stack", &[], &Mods::default(), ctx.tokens, nodes))
        }
    }
}

/// Emits every item of a `ForEach` (List body) directly into `out`, path-
/// tagged at its REAL tree position (`prefix` + `base + out.len()`), so the
/// items are honest direct children of whatever node receives `out`.
fn emit_for_each_items<'p>(
    ctx: &mut EmitCtx<'_, 'p>,
    for_each: ir::for_each::Reader<'p>,
    prefix: &[u32],
    base: u32,
    out: &mut Vec<LayoutNode>,
) -> Result<(), RtError> {
    let binding = for_each.get_binding().map_err(|_| RtError::Malformed)?;
    ctx.record_deps(binding, Damage::Layout);
    let items = match ctx.eval(binding)? {
        Value::List(items) => items,
        _ => return Err(RtError::TypeMismatch),
    };
    let slot = for_each.get_bind_slot() as usize;
    let template = for_each.get_template().map_err(|_| RtError::Malformed)?;
    let key_expr = for_each.get_key_expr().map_err(|_| RtError::Malformed)?;
    let windowed = for_each.get_windowed();
    let mut seen_keys: Vec<Vec<u8>> = Vec::with_capacity(items.len());
    for item in items {
        *ctx.locals.get_mut(slot).ok_or(RtError::MissingLocal)? = Some(item);
        if windowed {
            // Keyed identity: evaluate the key and enforce uniqueness
            // (stable ids for the retained tree; duplicate keys would
            // silently corrupt instance state).
            let key = ctx.eval(key_expr)?;
            let mut bytes = Vec::new();
            key.key_bytes(&mut bytes);
            if seen_keys.contains(&bytes) {
                return Err(RtError::DuplicateKey);
            }
            seen_keys.push(bytes);
        }
        for &seg in prefix {
            ctx.path.push(seg);
        }
        ctx.path.push(base + out.len() as u32);
        let emitted = emit_view(ctx, template)?;
        ctx.path.pop();
        for _ in prefix {
            ctx.path.pop();
        }
        out.push(emitted);
    }
    Ok(())
}

fn emit_widget<'p>(
    ctx: &mut EmitCtx<'_, 'p>,
    widget: ir::widget::Reader<'p>,
) -> Result<LayoutNode, RtError> {
    let kind = String::from(ctx.symbol(widget.get_kind()));

    // Props (layout class — text/content changes measurement).
    let prop_list = widget.get_props().map_err(|_| RtError::Malformed)?;
    let mut props: Vec<(String, Value)> = Vec::with_capacity(prop_list.len() as usize);
    for prop in prop_list.iter() {
        let expr = prop.get_value().map_err(|_| RtError::Malformed)?;
        ctx.record_deps(expr, Damage::Layout);
        let value = ctx.eval(expr)?;
        props.push((String::from(ctx.symbol(prop.get_name())), value));
    }

    // Modifiers.
    let mut mods = Mods::default();
    for modifier in widget.get_modifiers().map_err(|_| RtError::Malformed)?.iter() {
        apply_modifier(ctx, modifier, &mut mods)?;
    }

    // Handlers: capture at emit time (payloads snapshot the current state /
    // loop bindings — see module docs). Disabled nodes take no input.
    if !mods.disabled {
        for handler in widget.get_handlers().map_err(|_| RtError::Malformed)?.iter() {
            use ir::handler::Which;
            let action = match handler.which() {
                Ok(Which::Dispatch(Ok(dispatch))) => {
                    let payload_list = dispatch.get_payload().map_err(|_| RtError::Malformed)?;
                    let mut payload = Vec::with_capacity(payload_list.len() as usize);
                    for arg in payload_list.iter() {
                        // Payload state reads: Paint-class dep so any change
                        // re-emits and re-captures the payload.
                        ctx.record_deps(arg, Damage::Paint);
                        payload.push(ctx.eval(arg)?);
                    }
                    Some(HandlerAction::Dispatch {
                        event: dispatch.get_event(),
                        case: dispatch.get_case(),
                        payload,
                    })
                }
                Ok(Which::Navigate(Ok(path_expr))) => {
                    ctx.record_deps(path_expr, Damage::Paint);
                    match ctx.eval(path_expr)? {
                        crate::store::Value::Str(path) => Some(HandlerAction::Navigate { path }),
                        _ => return Err(RtError::TypeMismatch),
                    }
                }
                Ok(Which::Bind(Ok(get))) => {
                    let path_list = get.get_path().map_err(|_| RtError::Malformed)?;
                    let mut path = Vec::with_capacity(path_list.len() as usize);
                    for i in 0..path_list.len() {
                        path.push(path_list.get(i));
                    }
                    Some(HandlerAction::Bind { store: get.get_store(), path })
                }
                // emitProp handlers route through component instances — wired
                // with the instance/params work (see the task ledger).
                _ => None,
            };
            if let Some(action) = action {
                ctx.handlers.push(HandlerEntry {
                    path: ctx.path.clone(),
                    trigger: handler.get_trigger(),
                    action,
                    press_offset: registry::press_offset(&kind),
                });
            }
        }
    }

    // Children. Their final-tree position depends on the kit builder's
    // structure (registry::child_path) — prefix + base keep handler/text
    // paths honest.
    let child_list = widget.get_children().map_err(|_| RtError::Malformed)?;
    let (prefix, base) = registry::child_path(&kind);
    let mut children = Vec::with_capacity(child_list.len() as usize);
    for child in child_list.iter() {
        // A ForEach child SPLICES its items into THIS widget (no wrapper box
        // — a wrapper reset the parent's row/wrap flex context: the grid
        // rendered as a column). Paths are tagged at the real positions.
        if let Ok(ir::view_node::Which::ForEach(Ok(fe))) = child.which() {
            emit_for_each_items(ctx, fe, prefix, base, &mut children)?;
            continue;
        }
        // A Slot child SPLICES the caller's body into THIS widget for the same
        // reason (RFC-0084 §5): a wrapper would reset the flex context, so
        // `Stack { Slot content }.grow(1)` would stop growing the moment the
        // caller passed more than one node. An unbound slot adds nothing.
        if let Ok(ir::view_node::Which::Slot(Ok(slot_ref))) = child.which() {
            emit_slot_items(ctx, slot_ref.get_slot(), prefix, base, &mut children)?;
            continue;
        }
        for &seg in prefix {
            ctx.path.push(seg);
        }
        ctx.path.push(base + children.len() as u32);
        let emitted = emit_view(ctx, child)?;
        ctx.path.pop();
        for _ in prefix {
            ctx.path.pop();
        }
        children.push(emitted);
    }

    // Inherently-animated KIT WIDGET: emit a CONTINUOUS loop intent for this
    // node (paint-time transform loop — the app-host heap never frees, so
    // per-frame re-emit is out). The intent VALUE carries the loop sub-kind
    // (`crate::anim::LOOP_*`); the animated part is a stable pre-order child
    // offset of the builder's fixed structure, so the host targets child
    // boxes without any rebuild.
    if let Some(sub) = loop_subkind(&kind, &props) {
        ctx.anim_intents.push((
            ctx.path.clone(),
            AnimIntent::new(AnimKind::Loop, animation::MotionToken::Pulse.id(), sub),
        ));
    }
    Ok(registry::build_widget(&kind, &props, &mods, ctx.tokens, children))
}

/// Continuous-loop sub-kind for inherently-animated kit widgets (None = the
/// widget does not loop). Mirrors the registry arms: a `ProgressBar` is
/// indeterminate exactly when it has no Int `value` prop.
fn loop_subkind(kind: &str, props: &[(String, Value)]) -> Option<i32> {
    match kind {
        "Skeleton" => Some(crate::anim::LOOP_SWEEP),
        "Spinner" => Some(crate::anim::LOOP_CAROUSEL),
        "ProgressBar" if !props.iter().any(|(n, v)| n == "value" && matches!(v, Value::Int(_))) => {
            Some(crate::anim::LOOP_SWEEP)
        }
        _ => None,
    }
}

/// Stamps a motion intent (`.animate`/`.transition`/`.effect`) onto the
/// current node's path. Reads the first argument as the curated motion token
/// (resolved to its stable `animation::MotionToken` id) and, for
/// animate/effect, evaluates the second argument as the driving `value:` /
/// `trigger:` expression — recording it as a PAINT dependency and snapshotting
/// the committed value so the host can detect a change on the next emit. An
/// unknown token (the checker rejects it) or a missing value is a no-op.
pub(super) fn stamp_anim(
    ctx: &mut EmitCtx<'_, '_>,
    args: capnp::struct_list::Reader<'_, ir::token_arg::Owned>,
    kind: AnimKind,
) {
    let token_name = match args.get(0).which() {
        Ok(ir::token_arg::Which::Token(sym)) => alloc::string::String::from(ctx.symbol(sym)),
        _ => return,
    };
    let Some(token) = animation::MotionToken::from_name(&token_name).map(|t| t.id()) else {
        return;
    };
    // Driving value snapshot (animate/effect carry a second expr arg; a
    // transition is a lifecycle event with no expr → "present" = 1).
    let value = match kind {
        // `Loop` is never authored via a modifier (the runtime emits it for a
        // widget); `stamp_anim` only handles the modifier kinds.
        AnimKind::Loop => return,
        AnimKind::Transition => 1,
        AnimKind::Animate | AnimKind::Effect => {
            let Some(second) = (args.len() > 1).then(|| args.get(1)) else { return };
            match second.which() {
                Ok(ir::token_arg::Which::Expr(Ok(expr))) => {
                    ctx.record_deps(expr, Damage::Paint);
                    match ctx.eval(expr) {
                        Ok(v) => snapshot_i32(&v),
                        Err(_) => return,
                    }
                }
                Ok(ir::token_arg::Which::Boolean(b)) => i32::from(b),
                Ok(ir::token_arg::Which::Int(i)) => {
                    i.clamp(i32::MIN as i64, i32::MAX as i64) as i32
                }
                _ => return,
            }
        }
    };
    ctx.anim_intents.push((ctx.path.clone(), AnimIntent::new(kind, token, value)));
}

/// Collapses a committed [`Value`] to the `i32` change-detector the host diffs.
fn snapshot_i32(v: &Value) -> i32 {
    match v {
        Value::Bool(b) => i32::from(*b),
        Value::Int(i) => (*i).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        Value::Fx(raw) => (raw >> 32).clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        _ => 0,
    }
}
