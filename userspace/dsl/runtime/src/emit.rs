// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! View emission: IR view tree + committed state → `LayoutNode` scene,
//! recording state→node dependencies with their invalidation class.

use crate::interact::{HandlerAction, HandlerEntry};
use crate::reduce::{eval, EvalCtx};
use crate::registry::{self, Mods};
use crate::store::{StoreState, Value};
use crate::{DeviceEnv, LocaleSource, RtError};
use alloc::{string::String, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;
use nexus_layout_types::{Align, Direction, EdgeInsets, Justify, LayoutNode};
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

pub(crate) struct EmitCtx<'a> {
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
    /// Absolute child-index path of the node currently being emitted.
    pub path: Vec<u32>,
    pub components: capnp::struct_list::Reader<'a, ir::component::Owned>,
}

impl EmitCtx<'_> {
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
                    if path.len() > 0 {
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
pub(crate) fn emit_view(
    ctx: &mut EmitCtx<'_>,
    node: ir::view_node::Reader<'_>,
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
            let mut chosen: Option<
                capnp::struct_list::Reader<'_, ir::view_node::Owned>,
            > = None;
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
            let component = ctx
                .components
                .get(component_ref.get_component());
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
                path,
                components: ctx.components,
            };
            emit_view(&mut inner, view)
        }
    }
}

/// Emits every item of a `ForEach` (List body) directly into `out`, path-
/// tagged at its REAL tree position (`prefix` + `base + out.len()`), so the
/// items are honest direct children of whatever node receives `out`.
fn emit_for_each_items(
    ctx: &mut EmitCtx<'_>,
    for_each: ir::for_each::Reader<'_>,
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
            if seen_keys.iter().any(|k| *k == bytes) {
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

fn emit_widget(
    ctx: &mut EmitCtx<'_>,
    widget: ir::widget::Reader<'_>,
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
                    let payload_list =
                        dispatch.get_payload().map_err(|_| RtError::Malformed)?;
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
                        crate::store::Value::Str(path) => {
                            Some(HandlerAction::Navigate { path })
                        }
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

    Ok(registry::build_widget(&kind, &props, &mods, ctx.tokens, children))
}

/// Applies one modifier; mod ids index the compiler catalog
/// (`nexus-dsl-core::registry::MODIFIERS`) — matched here by stable id order.
fn apply_modifier(
    ctx: &mut EmitCtx<'_>,
    modifier: ir::modifier::Reader<'_>,
    mods: &mut Mods,
) -> Result<(), RtError> {
    let args = modifier.get_args().map_err(|_| RtError::Malformed)?;
    let first = args.iter().next();
    let token_name = |ctx: &EmitCtx<'_>| -> String {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Token(sym))) => String::from(ctx.symbol(sym)),
            _ => String::new(),
        }
    };
    let int_arg = || -> i64 {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => i,
            _ => 0,
        }
    };
    // Raw-px size argument (`.width(320)`); a token (`full`) yields None.
    let px_arg = || -> Option<nexus_layout_types::FxPx> {
        match first.map(|a| a.which()) {
            Some(Ok(ir::token_arg::Which::Int(i))) => Some(nexus_layout_types::FxPx::new(i.clamp(0, 16384) as i32)),
            _ => None,
        }
    };
    // Catalog order (docs/dev/dsl/modifiers.md); ids are stable.
    match modifier.get_mod_id() {
        0 => mods.padding = EdgeInsets::all(registry::spacing(int_arg())), // padding
        1 => {
            let px = registry::spacing(int_arg());
            mods.padding.left = px;
            mods.padding.right = px;
        } // paddingX
        2 => {
            let px = registry::spacing(int_arg());
            mods.padding.top = px;
            mods.padding.bottom = px;
        } // paddingY
        7 => mods.gap = registry::spacing(int_arg()),                      // gap
        // Sizes are RAW px (modifiers.md: "length token | full | Int px");
        // `full` stays a no-op (cross-axis children stretch by default).
        9 => mods.width = px_arg(),                            // width
        10 => mods.height = px_arg(),                          // height
        11 => mods.min_width = px_arg(),                       // minWidth
        12 => mods.max_width = px_arg(),                       // maxWidth
        13 => mods.min_height = px_arg(),                      // minHeight
        14 => mods.max_height = px_arg(),                      // maxHeight
        15 => mods.grow = int_arg().max(0) as u32,                          // grow
        16 => mods.shrink = Some(int_arg().max(0) as u32),                  // shrink
        18 => {
            mods.align = Some(match token_name(ctx).as_str() {
                "start" => Align::Start,
                "center" => Align::Center,
                "end" => Align::End,
                _ => Align::Stretch,
            });
        } // align
        19 => {
            mods.justify = Some(match token_name(ctx).as_str() {
                "center" => Justify::Center,
                "end" => Justify::End,
                "between" => Justify::SpaceBetween,
                _ => Justify::Start,
            });
        } // justify
        20 => {
            mods.direction = Some(match token_name(ctx).as_str() {
                "row" => Direction::Row,
                _ => Direction::Column,
            });
        } // direction
        24 => mods.bg = registry::color_token(&token_name(ctx)),           // bg
        25 => mods.fg = registry::color_token(&token_name(ctx)),           // fg
        27 => mods.opacity = Some(int_arg().clamp(0, 255) as u8),          // opacity
        28 => mods.material = registry::material_token(&token_name(ctx)),  // material
        29 => mods.rounded = Some(registry::radius(&token_name(ctx))),     // rounded
        32 => mods.text_size = registry::type_size(&token_name(ctx)),      // textSize
        21 => {
            if let Some(Ok(ir::token_arg::Which::Boolean(b))) = first.map(|a| a.which()) {
                mods.wrap = b;
            }
        } // wrap
        37 => {
            // disabled(bool | expr) — PAINT-class dependency.
            if let Some(Ok(which)) = first.map(|a| a.which()) {
                match which {
                    ir::token_arg::Which::Boolean(b) => mods.disabled = b,
                    ir::token_arg::Which::Expr(Ok(expr)) => {
                        ctx.record_deps(expr, Damage::Paint);
                        mods.disabled = matches!(ctx.eval(expr)?, Value::Bool(true));
                    }
                    _ => {}
                }
            }
        } // disabled
        _ => {} // key/label/others: identity/semantics — no paint effect here
    }
    Ok(())
}
