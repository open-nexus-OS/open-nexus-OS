// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Expression + statement + type lowering.

use super::{unsupported, Ctx};
use crate::ast::{AssignOp, BinOp, Expr, Stmt, TypeExpr, UnOp};
use crate::diag::Diagnostic;
use alloc::collections::BTreeMap;
use alloc::string::String;
use nexus_dsl_ir::ui_ir_capnp as ir;

/// Lexical environment for one body/view.
pub(super) struct Env<'a> {
    pub ctx: &'a Ctx<'a>,
    pub locals: BTreeMap<String, u32>,
    pub params: BTreeMap<String, u32>,
    pub next_slot: u32,
}

impl<'a> Env<'a> {
    pub(super) fn new(ctx: &'a Ctx<'a>) -> Self {
        Self { ctx, locals: BTreeMap::new(), params: BTreeMap::new(), next_slot: 0 }
    }

    pub(super) fn bind_local(&mut self, name: &str) -> u32 {
        let slot = self.next_slot;
        self.next_slot += 1;
        self.locals.insert(String::from(name), slot);
        slot
    }

    /// Binds a name to an existing slot (Ok/Err arms of one call share the
    /// result slot -- only one path ever runs).
    pub(super) fn bind_local_to(&mut self, name: &str, slot: u32) {
        self.locals.insert(String::from(name), slot);
    }
}

/// Lowers a type annotation. Unknown named types lower to `opaque` in v0.1
/// (service/domain schemas land in v0.2b and replace them).
pub(super) fn lower_type(ty: &TypeExpr, builder: ir::type_ref::Builder<'_>) {
    let mut builder = builder;
    match (ty.name.text.as_str(), ty.args.len()) {
        ("Bool", 0) => builder.set_bool(()),
        ("Int", 0) => builder.set_int(()),
        ("Fx", 0) => builder.set_fx(()),
        ("Str", 0) => builder.set_str(0),
        ("EventRef", 0) => builder.set_event_ref(()),
        ("List", 1) => {
            let mut list = builder.init_list();
            list.set_cap(0);
            lower_type(&ty.args[0], list.init_elem());
        }
        ("Option", 1) => {
            lower_type(&ty.args[0], builder.init_option());
        }
        _ => builder.set_opaque(()),
    }
}

fn set_opaque_type(expr: &mut ir::expr::Builder<'_>) {
    expr.reborrow().init_type().set_opaque(());
}

/// Lowers one expression into the given builder.
pub(super) fn lower_expr(
    env: &Env<'_>,
    expr: &Expr,
    builder: ir::expr::Builder<'_>,
) -> Result<(), Diagnostic> {
    let mut b = builder;
    match expr {
        Expr::Bool { value, .. } => {
            b.reborrow().init_type().set_bool(());
            b.set_lit_bool(*value);
        }
        Expr::Int { value, .. } => {
            b.reborrow().init_type().set_int(());
            b.set_lit_int(*value);
        }
        Expr::Fx { value, .. } => {
            b.reborrow().init_type().set_fx(());
            b.set_lit_fx(*value);
        }
        Expr::Str { value, .. } => {
            b.reborrow().init_type().set_str(0);
            b.set_lit_str(capnp::text::Reader::from(value.as_str()));
        }
        Expr::List { items, .. } => {
            set_opaque_type(&mut b);
            let mut list = b.init_lit_list(items.len() as u32);
            for (i, item) in items.iter().enumerate() {
                lower_expr(env, item, list.reborrow().get(i as u32))?;
            }
        }
        Expr::EnumLit { case, args, .. } => {
            set_opaque_type(&mut b);
            let case_ref =
                env.ctx.event_case(case.text.as_str()).unwrap_or((0, env.ctx.sym(&case.text)));
            let mut lit = b.init_lit_enum();
            lit.set_enum_type(case_ref.0);
            lit.set_case(case_ref.1);
            let mut payload = lit.init_payload(args.len() as u32);
            for (i, arg) in args.iter().enumerate() {
                lower_expr(env, arg, payload.reborrow().get(i as u32))?;
            }
        }
        Expr::StateRef { path, span } => {
            set_opaque_type(&mut b);
            let Some(first) = path.first() else {
                return Err(unsupported(*span, "empty `$state` path"));
            };
            // Multi-store: the field name resolves its owning store; a name
            // shared by several stores is ambiguous and must be renamed.
            let store = match env.ctx.store_of_field(&first.text) {
                Ok(store) => store,
                Err(true) => {
                    return Err(unsupported(
                        *span,
                        "a state field named in more than one store (rename it)",
                    ));
                }
                Err(false) => {
                    return Err(unsupported(*span, "a state field no store declares"));
                }
            };
            let mut get = b.init_field_get();
            get.set_store(store);
            let mut segs = get.init_path(path.len() as u32);
            for (i, seg) in path.iter().enumerate() {
                segs.set(i as u32, env.ctx.sym(&seg.text));
            }
        }
        Expr::PropsRef { path, span } => {
            let Some(first) = path.first() else {
                return Err(unsupported(*span, "empty `$props` path"));
            };
            let Some(&param) = env.params.get(first.text.as_str()) else {
                return Err(unsupported(*span, "`$props` outside a component"));
            };
            // $props.a.b.c → RecordGet chains around ParamGet.
            lower_access_chain(env, &mut b, AccessBase::Param(param), &path[1..])?;
        }
        Expr::DeviceRef { path, span } => {
            set_opaque_type(&mut b);
            let Some(first) = path.first() else {
                return Err(unsupported(*span, "empty `device` path"));
            };
            let field_id = crate::registry::DEVICE_FIELDS
                .iter()
                .position(|(name, _)| *name == first.text)
                .unwrap_or(0);
            b.set_device_get(field_id as u32);
        }
        Expr::Path { segments, span } => {
            let first = &segments[0];
            if let Some(&slot) = env.locals.get(first.text.as_str()) {
                lower_access_chain(env, &mut b, AccessBase::Local(slot), &segments[1..])?;
            } else if let Some(&param) = env.params.get(first.text.as_str()) {
                lower_access_chain(env, &mut b, AccessBase::Param(param), &segments[1..])?;
            } else if segments.len() == 1 {
                // Bare vocabulary word (device values, token-ish idents in
                // comparisons): lowers as a string literal, typed opaque.
                set_opaque_type(&mut b);
                b.set_lit_str(capnp::text::Reader::from(first.text.as_str()));
            } else {
                return Err(unsupported(*span, "unresolved path"));
            }
        }
        Expr::Call { path, args, span } => {
            // The one expression-position call in the v0.1 subset is the
            // `tail(list, n)` store-window builtin (keep the last n elements).
            // Everything else (svc.*) is lowered at effect-step level only.
            if path.len() == 1 && path[0].text == "tail" && args.len() == 2 {
                set_opaque_type(&mut b);
                let mut lop = b.init_list_op();
                lop.set_op(ir::ListOpKind::Tail);
                lower_expr(env, &args[0].value, lop.reborrow().init_base())?;
                lower_expr(env, &args[1].value, lop.init_arg())?;
            } else {
                return Err(unsupported(*span, "expression-position calls"));
            }
        }
        Expr::I18n { key, args, .. } => {
            set_opaque_type(&mut b);
            let mut fmt = b.init_fmt_i18n();
            fmt.set_key(env.ctx.i18n_index(key));
            let mut list = fmt.init_args(args.len() as u32);
            for (i, arg) in args.iter().enumerate() {
                lower_expr(env, arg, list.reborrow().get(i as u32))?;
            }
        }
        Expr::Unary { op, operand, .. } => {
            set_opaque_type(&mut b);
            let mut un = b.init_un_op();
            un.set_op(match op {
                UnOp::Not => ir::UnOpKind::Not,
                UnOp::Neg => ir::UnOpKind::Neg,
            });
            lower_expr(env, operand, un.init_operand())?;
        }
        Expr::Binary { op, lhs, rhs, .. } => {
            set_opaque_type(&mut b);
            let mut bin = b.init_bin_op();
            bin.set_op(match op {
                BinOp::Add => ir::BinOpKind::Add,
                BinOp::Sub => ir::BinOpKind::Sub,
                BinOp::Mul => ir::BinOpKind::Mul,
                BinOp::Div => ir::BinOpKind::Div,
                BinOp::Rem => ir::BinOpKind::Rem,
                BinOp::Eq => ir::BinOpKind::Eq,
                BinOp::Ne => ir::BinOpKind::Ne,
                BinOp::Lt => ir::BinOpKind::Lt,
                BinOp::Le => ir::BinOpKind::Le,
                BinOp::Gt => ir::BinOpKind::Gt,
                BinOp::Ge => ir::BinOpKind::Ge,
                BinOp::And => ir::BinOpKind::And,
                BinOp::Or => ir::BinOpKind::Or,
            });
            lower_expr(env, lhs, bin.reborrow().init_lhs())?;
            lower_expr(env, rhs, bin.init_rhs())?;
        }
    }
    Ok(())
}

enum AccessBase {
    Local(u32),
    Param(u32),
}

/// `base.f1.f2` → nested RecordGet around the base accessor.
fn lower_access_chain(
    env: &Env<'_>,
    builder: &mut ir::expr::Builder<'_>,
    base: AccessBase,
    fields: &[crate::ast::Ident],
) -> Result<(), Diagnostic> {
    set_opaque_type(builder);
    let Some((last, front)) = fields.split_last() else {
        match base {
            AccessBase::Local(slot) => builder.set_local_get(slot),
            AccessBase::Param(idx) => builder.set_param_get(idx),
        }
        return Ok(());
    };
    let mut get = builder.reborrow().init_record_get();
    get.set_field(env.ctx.sym(&last.text));
    let mut inner = get.init_base();
    lower_access_chain(env, &mut inner, base, front)?;
    Ok(())
}

/// Lowers reducer-body statements (pure subset).
pub(super) fn lower_stmts(
    env: &mut Env<'_>,
    stmts: &[Stmt],
    builder: capnp::struct_list::Builder<'_, ir::stmt::Owned>,
) -> Result<(), Diagnostic> {
    let mut list = builder;
    for (i, stmt) in stmts.iter().enumerate() {
        let b = list.reborrow().get(i as u32);
        match stmt {
            Stmt::Assign { path, op, value, .. } => {
                let mut set = b.init_set();
                set.set_op(match op {
                    AssignOp::Assign => ir::AssignOp::Assign,
                    AssignOp::AddAssign => ir::AssignOp::AddAssign,
                    AssignOp::SubAssign => ir::AssignOp::SubAssign,
                });
                {
                    let mut segs = set.reborrow().init_path(path.len() as u32);
                    for (j, seg) in path.iter().enumerate() {
                        segs.set(j as u32, env.ctx.sym(&seg.text));
                    }
                }
                lower_expr(env, value, set.init_value())?;
            }
            Stmt::Let { name, value, .. } => {
                let mut let_stmt = b.init_let_local();
                lower_expr(env, value, let_stmt.reborrow().init_value())?;
                let slot = env.bind_local(&name.text);
                let_stmt.set_slot(slot);
            }
            Stmt::If { cond, then, els, .. } => {
                let mut if_stmt = b.init_if_else();
                lower_expr(env, cond, if_stmt.reborrow().init_cond())?;
                lower_stmts(env, then, if_stmt.reborrow().init_then(then.len() as u32))?;
                lower_stmts(env, els, if_stmt.init_else(els.len() as u32))?;
            }
            Stmt::Match { scrutinee, arms, .. } => {
                let mut match_stmt = b.init_match_enum();
                lower_expr(env, scrutinee, match_stmt.reborrow().init_scrutinee())?;
                let mut arm_list = match_stmt.init_arms(arms.len() as u32);
                for (j, arm) in arms.iter().enumerate() {
                    let mut arm_b = arm_list.reborrow().get(j as u32);
                    let case_idx = env
                        .ctx
                        .event_case(arm.pattern.case.text.as_str())
                        .map(|(_, c)| c)
                        .unwrap_or(0);
                    arm_b.set_case(case_idx);
                    {
                        let slots: alloc::vec::Vec<u32> = arm
                            .pattern
                            .binds
                            .iter()
                            .map(|bind| env.bind_local(&bind.text))
                            .collect();
                        let mut binds = arm_b.reborrow().init_binds(slots.len() as u32);
                        for (k, slot) in slots.iter().enumerate() {
                            binds.set(k as u32, *slot);
                        }
                    }
                    lower_stmts(env, &arm.body, arm_b.init_body(arm.body.len() as u32))?;
                }
            }
            Stmt::Dispatch { span, .. } | Stmt::ExprStmt { span, .. } => {
                return Err(unsupported(*span, "IO statements in a pure body"));
            }
        }
    }
    Ok(())
}
