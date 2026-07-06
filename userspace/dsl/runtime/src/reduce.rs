// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Expression evaluation + pure statement execution (the reducer engine).
//!
//! Walks the IR expression trees directly over Cap'n Proto readers — no
//! intermediate representation, no compilation. Arithmetic is checked
//! (overflow = deterministic error, never wraparound); `Fx` math widens
//! through i128. Everything is total: the tree shape bounds the work.

use crate::store::{StoreState, Value};
use crate::{DeviceEnv, LocaleSource, RtError};
use alloc::{string::String, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;

/// Read-only evaluation context.
pub(crate) struct EvalCtx<'a> {
    pub stores: &'a [StoreState],
    pub locals: &'a mut [Option<Value>],
    pub params: &'a [Value],
    pub device: &'a dyn DeviceEnv,
    pub locale: &'a dyn LocaleSource,
}

pub(crate) fn eval(ctx: &mut EvalCtx<'_>, expr: ir::expr::Reader<'_>) -> Result<Value, RtError> {
    use ir::expr::Which;
    match expr.which().map_err(|_| RtError::Malformed)? {
        Which::LitBool(b) => Ok(Value::Bool(b)),
        Which::LitInt(i) => Ok(Value::Int(i)),
        Which::LitFx(f) => Ok(Value::Fx(f)),
        Which::LitStr(s) => Ok(Value::Str(String::from(
            s.map_err(|_| RtError::Malformed)?.to_str().map_err(|_| RtError::Malformed)?,
        ))),
        Which::LitList(items) => {
            let items = items.map_err(|_| RtError::Malformed)?;
            let mut out = Vec::with_capacity(items.len() as usize);
            for item in items.iter() {
                out.push(eval(ctx, item)?);
            }
            Ok(Value::List(out))
        }
        Which::LitEnum(lit) => {
            let lit = lit.map_err(|_| RtError::Malformed)?;
            let payload_list = lit.get_payload().map_err(|_| RtError::Malformed)?;
            let mut payload = Vec::with_capacity(payload_list.len() as usize);
            for item in payload_list.iter() {
                payload.push(eval(ctx, item)?);
            }
            Ok(Value::Enum { event: lit.get_enum_type(), case: lit.get_case(), payload })
        }
        Which::FieldGet(get) => {
            let get = get.map_err(|_| RtError::Malformed)?;
            let store =
                ctx.stores.get(get.get_store() as usize).ok_or(RtError::UnknownField)?;
            let path = get.get_path().map_err(|_| RtError::Malformed)?;
            if path.is_empty() {
                return Err(RtError::UnknownField);
            }
            let index = store.field_index(path.get(0))?;
            let mut value = store.get(index)?;
            for i in 1..path.len() {
                let field = path.get(i);
                match value {
                    Value::Record(fields) => {
                        value = fields
                            .iter()
                            .find(|(sym, _)| *sym == field)
                            .map(|(_, v)| v)
                            .ok_or(RtError::UnknownField)?;
                    }
                    _ => return Err(RtError::TypeMismatch),
                }
            }
            Ok(value.clone())
        }
        Which::LocalGet(slot) => ctx
            .locals
            .get(slot as usize)
            .and_then(|v| v.clone())
            .ok_or(RtError::MissingLocal),
        Which::ParamGet(index) => {
            ctx.params.get(index as usize).cloned().ok_or(RtError::MissingLocal)
        }
        Which::RecordGet(get) => {
            let get = get.map_err(|_| RtError::Malformed)?;
            let base = eval(ctx, get.get_base().map_err(|_| RtError::Malformed)?)?;
            let field = get.get_field();
            match base {
                Value::Record(fields) => fields
                    .into_iter()
                    .find(|(sym, _)| *sym == field)
                    .map(|(_, v)| v)
                    .ok_or(RtError::UnknownField),
                _ => Err(RtError::TypeMismatch),
            }
        }
        Which::RecordMake(fields) => {
            let fields = fields.map_err(|_| RtError::Malformed)?;
            let mut out = Vec::with_capacity(fields.len() as usize);
            for (i, field) in fields.iter().enumerate() {
                out.push((i as u32, eval(ctx, field)?));
            }
            Ok(Value::Record(out))
        }
        Which::UnOp(un) => {
            let un = un.map_err(|_| RtError::Malformed)?;
            let operand = eval(ctx, un.get_operand().map_err(|_| RtError::Malformed)?)?;
            match (un.get_op().map_err(|_| RtError::Malformed)?, operand) {
                (ir::UnOpKind::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
                (ir::UnOpKind::Neg, Value::Int(i)) => {
                    i.checked_neg().map(Value::Int).ok_or(RtError::Overflow)
                }
                (ir::UnOpKind::Neg, Value::Fx(f)) => {
                    f.checked_neg().map(Value::Fx).ok_or(RtError::Overflow)
                }
                _ => Err(RtError::TypeMismatch),
            }
        }
        Which::BinOp(bin) => {
            let bin = bin.map_err(|_| RtError::Malformed)?;
            let op = bin.get_op().map_err(|_| RtError::Malformed)?;
            let lhs = eval(ctx, bin.get_lhs().map_err(|_| RtError::Malformed)?)?;
            // Short-circuit logic.
            if let (ir::BinOpKind::And, Value::Bool(false)) = (op, &lhs) {
                return Ok(Value::Bool(false));
            }
            if let (ir::BinOpKind::Or, Value::Bool(true)) = (op, &lhs) {
                return Ok(Value::Bool(true));
            }
            let rhs = eval(ctx, bin.get_rhs().map_err(|_| RtError::Malformed)?)?;
            binop(op, lhs, rhs)
        }
        Which::FmtI18n(fmt) => {
            let fmt = fmt.map_err(|_| RtError::Malformed)?;
            let args_list = fmt.get_args().map_err(|_| RtError::Malformed)?;
            let mut args = Vec::with_capacity(args_list.len() as usize);
            for arg in args_list.iter() {
                args.push(eval(ctx, arg)?);
            }
            Ok(Value::Str(ctx.locale.format(fmt.get_key(), &args)))
        }
        Which::DeviceGet(field) => Ok(ctx.device.get(field)),
        Which::OptionSome(inner) => eval(ctx, inner.map_err(|_| RtError::Malformed)?),
        Which::OptionNone(()) => Ok(Value::Unit),
        Which::ListOp(_) => Err(RtError::Unsupported), // combinators land in v0.2
    }
}

fn binop(op: ir::BinOpKind, lhs: Value, rhs: Value) -> Result<Value, RtError> {
    use ir::BinOpKind as K;
    match op {
        K::And | K::Or => match (lhs, rhs) {
            (Value::Bool(a), Value::Bool(b)) => {
                Ok(Value::Bool(if op == K::And { a && b } else { a || b }))
            }
            _ => Err(RtError::TypeMismatch),
        },
        K::Eq => Ok(Value::Bool(lhs == rhs)),
        K::Ne => Ok(Value::Bool(lhs != rhs)),
        K::Lt | K::Le | K::Gt | K::Ge => {
            let ordering = match (&lhs, &rhs) {
                (Value::Int(a), Value::Int(b)) => a.cmp(b),
                (Value::Fx(a), Value::Fx(b)) => a.cmp(b),
                (Value::Str(a), Value::Str(b)) => a.cmp(b),
                _ => return Err(RtError::TypeMismatch),
            };
            Ok(Value::Bool(match op {
                K::Lt => ordering.is_lt(),
                K::Le => ordering.is_le(),
                K::Gt => ordering.is_gt(),
                _ => ordering.is_ge(),
            }))
        }
        K::Add | K::StrConcat => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                a.checked_add(b).map(Value::Int).ok_or(RtError::Overflow)
            }
            (Value::Fx(a), Value::Fx(b)) => {
                a.checked_add(b).map(Value::Fx).ok_or(RtError::Overflow)
            }
            (Value::Str(mut a), Value::Str(b)) => {
                a.push_str(&b);
                Ok(Value::Str(a))
            }
            _ => Err(RtError::TypeMismatch),
        },
        K::Sub => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                a.checked_sub(b).map(Value::Int).ok_or(RtError::Overflow)
            }
            (Value::Fx(a), Value::Fx(b)) => {
                a.checked_sub(b).map(Value::Fx).ok_or(RtError::Overflow)
            }
            _ => Err(RtError::TypeMismatch),
        },
        K::Mul => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                a.checked_mul(b).map(Value::Int).ok_or(RtError::Overflow)
            }
            (Value::Fx(a), Value::Fx(b)) => {
                let wide = (i128::from(a) * i128::from(b)) >> 32;
                i64::try_from(wide).map(Value::Fx).map_err(|_| RtError::Overflow)
            }
            _ => Err(RtError::TypeMismatch),
        },
        K::Div => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    Err(RtError::DivByZero)
                } else {
                    a.checked_div(b).map(Value::Int).ok_or(RtError::Overflow)
                }
            }
            (Value::Fx(a), Value::Fx(b)) => {
                if b == 0 {
                    Err(RtError::DivByZero)
                } else {
                    let wide = (i128::from(a) << 32) / i128::from(b);
                    i64::try_from(wide).map(Value::Fx).map_err(|_| RtError::Overflow)
                }
            }
            _ => Err(RtError::TypeMismatch),
        },
        K::Rem => match (lhs, rhs) {
            (Value::Int(a), Value::Int(b)) => {
                if b == 0 {
                    Err(RtError::DivByZero)
                } else {
                    a.checked_rem(b).map(Value::Int).ok_or(RtError::Overflow)
                }
            }
            _ => Err(RtError::TypeMismatch),
        },
    }
}

/// Executes pure statements against one store (the reducer path).
pub(crate) struct ExecCtx<'a> {
    pub store_index: usize,
    pub stores: &'a mut [StoreState],
    pub locals: &'a mut [Option<Value>],
    pub params: &'a [Value],
    pub device: &'a dyn DeviceEnv,
    pub locale: &'a dyn LocaleSource,
}

pub(crate) fn exec(
    ctx: &mut ExecCtx<'_>,
    stmts: capnp::struct_list::Reader<'_, ir::stmt::Owned>,
) -> Result<(), RtError> {
    use ir::stmt::Which;
    for stmt in stmts.iter() {
        match stmt.which().map_err(|_| RtError::Malformed)? {
            Which::Set(set) => {
                let set = set.map_err(|_| RtError::Malformed)?;
                let value = {
                    let mut eval_ctx = EvalCtx {
                        stores: ctx.stores,
                        locals: ctx.locals,
                        params: ctx.params,
                        device: ctx.device,
                        locale: ctx.locale,
                    };
                    eval(&mut eval_ctx, set.get_value().map_err(|_| RtError::Malformed)?)?
                };
                let path_list = set.get_path().map_err(|_| RtError::Malformed)?;
                let mut path = Vec::with_capacity(path_list.len() as usize);
                for i in 0..path_list.len() {
                    path.push(path_list.get(i));
                }
                let store =
                    ctx.stores.get_mut(ctx.store_index).ok_or(RtError::UnknownField)?;
                let op = set.get_op().map_err(|_| RtError::Malformed)?;
                let final_value = match op {
                    ir::AssignOp::Assign => value,
                    ir::AssignOp::AddAssign | ir::AssignOp::SubAssign => {
                        let index = store.field_index(path[0])?;
                        let current = store.get(index)?.clone();
                        let kind = if op == ir::AssignOp::AddAssign { ir::BinOpKind::Add } else { ir::BinOpKind::Sub };
                        binop(kind, current, value)?
                    }
                };
                store.set_path(&path, final_value)?;
            }
            Which::LetLocal(let_stmt) => {
                let let_stmt = let_stmt.map_err(|_| RtError::Malformed)?;
                let value = {
                    let mut eval_ctx = EvalCtx {
                        stores: ctx.stores,
                        locals: ctx.locals,
                        params: ctx.params,
                        device: ctx.device,
                        locale: ctx.locale,
                    };
                    eval(&mut eval_ctx, let_stmt.get_value().map_err(|_| RtError::Malformed)?)?
                };
                let slot = let_stmt.get_slot() as usize;
                *ctx.locals.get_mut(slot).ok_or(RtError::MissingLocal)? = Some(value);
            }
            Which::IfElse(if_stmt) => {
                let if_stmt = if_stmt.map_err(|_| RtError::Malformed)?;
                let cond = {
                    let mut eval_ctx = EvalCtx {
                        stores: ctx.stores,
                        locals: ctx.locals,
                        params: ctx.params,
                        device: ctx.device,
                        locale: ctx.locale,
                    };
                    eval(&mut eval_ctx, if_stmt.get_cond().map_err(|_| RtError::Malformed)?)?
                };
                let branch = match cond {
                    Value::Bool(true) => if_stmt.get_then().map_err(|_| RtError::Malformed)?,
                    Value::Bool(false) => if_stmt.get_else().map_err(|_| RtError::Malformed)?,
                    _ => return Err(RtError::TypeMismatch),
                };
                exec(ctx, branch)?;
            }
            Which::MatchEnum(match_stmt) => {
                let match_stmt = match_stmt.map_err(|_| RtError::Malformed)?;
                let scrutinee = {
                    let mut eval_ctx = EvalCtx {
                        stores: ctx.stores,
                        locals: ctx.locals,
                        params: ctx.params,
                        device: ctx.device,
                        locale: ctx.locale,
                    };
                    eval(
                        &mut eval_ctx,
                        match_stmt.get_scrutinee().map_err(|_| RtError::Malformed)?,
                    )?
                };
                let Value::Enum { case, payload, .. } = scrutinee else {
                    return Err(RtError::TypeMismatch);
                };
                for arm in match_stmt.get_arms().map_err(|_| RtError::Malformed)?.iter() {
                    if arm.get_case() == case {
                        let binds = arm.get_binds().map_err(|_| RtError::Malformed)?;
                        for (i, value) in payload.into_iter().enumerate() {
                            if i < binds.len() as usize {
                                let slot = binds.get(i as u32) as usize;
                                *ctx.locals.get_mut(slot).ok_or(RtError::MissingLocal)? =
                                    Some(value);
                            }
                        }
                        exec(ctx, arm.get_body().map_err(|_| RtError::Malformed)?)?;
                        break;
                    }
                }
            }
        }
    }
    Ok(())
}
