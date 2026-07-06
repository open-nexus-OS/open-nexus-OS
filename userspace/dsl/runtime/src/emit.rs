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
            let mut children = Vec::with_capacity(body.len() as usize);
            for (j, child) in body.iter().enumerate() {
                ctx.path.push(j as u32);
                let emitted = emit_view(ctx, child)?;
                ctx.path.pop();
                children.push(emitted);
            }
            // A transparent column wrapper keeps branch output a single node.
            Ok(registry::build_widget("Stack", &[], &Mods::default(), ctx.tokens, children))
        }
        Which::ForEach(for_each) => {
            let for_each = for_each.map_err(|_| RtError::Malformed)?;
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
            let mut children = Vec::with_capacity(items.len());
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
                ctx.path.push(children.len() as u32);
                let emitted = emit_view(ctx, template)?;
                ctx.path.pop();
                children.push(emitted);
            }
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

    // Children.
    let child_list = widget.get_children().map_err(|_| RtError::Malformed)?;
    let base = registry::child_base_offset(&kind);
    let mut children = Vec::with_capacity(child_list.len() as usize);
    for (k, child) in child_list.iter().enumerate() {
        ctx.path.push(base + k as u32);
        let emitted = emit_view(ctx, child)?;
        ctx.path.pop();
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
        29 => mods.rounded = Some(registry::radius(&token_name(ctx))),     // rounded
        32 => mods.text_size = registry::type_size(&token_name(ctx)),      // textSize
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
