// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Effect-plan execution (the only place IO happens).
//!
//! Written semantics (docs/dev/dsl/ir.md): steps run in order; a `call` step
//! binds its result slot on Ok and **continues**, dispatches `onErr` (if set)
//! and **stops** on Err; `dispatch` steps enqueue follow-up events. Effects
//! never mutate state directly — everything re-enters through the queue.

use crate::reduce::{eval, EvalCtx};
use crate::store::{StoreState, Value};
use crate::{DeviceEnv, EffectHost, LocaleSource, QueryCall, RtError};
use alloc::{string::String, vec::Vec};
use nexus_dsl_ir::ui_ir_capnp as ir;

/// A queued follow-up dispatch produced by an effect.
pub(crate) struct Pending {
    pub event: u32,
    pub case: u32,
    pub payload: Vec<Value>,
    /// The (event, case, generation) of the trigger whose plan enqueued this.
    /// `None` = a root dispatch (user/handler) — never cancelled.
    pub origin: Option<(u32, u32, u32)>,
}

pub(crate) struct EffectCtx<'a> {
    /// The running plan's trigger identity + generation (for cancellation).
    pub origin: (u32, u32, u32),
    pub stores: &'a [StoreState],
    pub locals: &'a mut [Option<Value>],
    pub device: &'a dyn DeviceEnv,
    pub locale: &'a dyn LocaleSource,
    pub host: &'a mut dyn EffectHost,
    pub symbols: &'a [alloc::string::String],
    /// The program root (query steps read `querySpecs` from it).
    pub root: ir::ui_program::Reader<'a>,
}

pub(crate) fn run_plan(
    ctx: &mut EffectCtx<'_>,
    plan: ir::effect_plan::Reader<'_>,
    trigger_payload: &[Value],
    queue: &mut Vec<Pending>,
) -> Result<(), RtError> {
    // Bind the trigger payload.
    let binds = plan.get_binds().map_err(|_| RtError::Malformed)?;
    for (i, value) in trigger_payload.iter().enumerate() {
        if i < binds.len() as usize {
            let slot = binds.get(i as u32) as usize;
            *ctx.locals.get_mut(slot).ok_or(RtError::MissingLocal)? = Some(value.clone());
        }
    }

    for step in plan.get_steps().map_err(|_| RtError::Malformed)?.iter() {
        use ir::effect_step::Which;
        match step.which().map_err(|_| RtError::Malformed)? {
            Which::Call(call) => {
                let call = call.map_err(|_| RtError::Malformed)?;
                let service = symbol(ctx.symbols, call.get_service())?;
                let method = symbol(ctx.symbols, call.get_method())?;
                let args_list = call.get_args().map_err(|_| RtError::Malformed)?;
                let mut args = Vec::with_capacity(args_list.len() as usize);
                for arg in args_list.iter() {
                    args.push(eval_in(ctx, arg)?);
                }
                match ctx.host.call(service, method, &args, call.get_timeout_ms()) {
                    Ok(result) => {
                        let slot = call.get_result_slot();
                        if slot != u32::MAX {
                            *ctx.locals.get_mut(slot as usize).ok_or(RtError::MissingLocal)? =
                                Some(result.clone());
                        }
                        if call.has_on_ok() {
                            let ok = call.get_on_ok().map_err(|_| RtError::Malformed)?;
                            enqueue(ctx, ok, queue)?;
                        }
                    }
                    Err(code) => {
                        if call.has_on_err() {
                            // The error code is available to the onErr payload
                            // via a reserved local (slot = resultSlot).
                            let slot = call.get_result_slot();
                            if slot != u32::MAX {
                                *ctx.locals
                                    .get_mut(slot as usize)
                                    .ok_or(RtError::MissingLocal)? =
                                    Some(Value::Int(i64::from(code)));
                            }
                            let err = call.get_on_err().map_err(|_| RtError::Malformed)?;
                            enqueue(ctx, err, queue)?;
                        }
                        return Ok(()); // stop the plan on Err — by contract
                    }
                }
            }
            Which::Dispatch(dispatch) => {
                let dispatch = dispatch.map_err(|_| RtError::Malformed)?;
                enqueue(ctx, dispatch, queue)?;
            }
            Which::Query(qstep) => {
                let qstep = qstep.map_err(|_| RtError::Malformed)?;
                if !run_query_step(ctx, qstep, queue)? {
                    return Ok(()); // stop the plan on Err — by contract
                }
            }
        }
    }
    Ok(())
}

/// Executes one query step: resolve the spec against the step's args, hand
/// the flattened [`QueryCall`] to the host, bind rows/next (Ok) or the error
/// code (Err), enqueue the follow-up dispatch. Returns `false` when the plan
/// must stop (the Err path).
fn run_query_step(
    ctx: &mut EffectCtx<'_>,
    qstep: ir::query_step::Reader<'_>,
    queue: &mut Vec<Pending>,
) -> Result<bool, RtError> {
    let specs = ctx.root.get_query_specs().map_err(|_| RtError::Malformed)?;
    let spec_idx = qstep.get_spec();
    if spec_idx >= specs.len() {
        return Err(RtError::Malformed);
    }
    let spec = specs.get(spec_idx);

    // Query params: the step's arg expressions, declaration order.
    let args_list = qstep.get_args().map_err(|_| RtError::Malformed)?;
    let mut params = Vec::with_capacity(args_list.len() as usize);
    for arg in args_list.iter() {
        params.push(eval_in(ctx, arg)?);
    }
    let token = match eval_in(ctx, qstep.get_token().map_err(|_| RtError::Malformed)?)? {
        Value::Str(s) => s,
        _ => return Err(RtError::TypeMismatch),
    };

    // Flatten predicates (values eval against the query params).
    let mut call = QueryCall {
        source: String::from(symbol(ctx.symbols, spec.get_source())?),
        eq: Vec::new(),
        low: None,
        high: None,
        order_col: String::from(symbol(ctx.symbols, spec.get_order_col())?),
        descending: spec.get_descending(),
        limit: spec.get_limit(),
        token,
    };
    for pred in spec.get_preds().map_err(|_| RtError::Malformed)?.iter() {
        let value = {
            let mut eval_ctx = EvalCtx {
                stores: ctx.stores,
                locals: ctx.locals,
                params: &params,
                device: ctx.device,
                locale: ctx.locale,
            };
            eval(&mut eval_ctx, pred.get_value().map_err(|_| RtError::Malformed)?)?
        };
        let col = String::from(symbol(ctx.symbols, pred.get_col())?);
        match pred.get_op().map_err(|_| RtError::Malformed)? {
            ir::QueryOp::Eq => call.eq.push((col, value)),
            ir::QueryOp::Ge => call.low = Some(value),
            ir::QueryOp::Le => call.high = Some(value),
        }
    }

    match ctx.host.query(&call) {
        Ok(page) => {
            let rows_slot = qstep.get_rows_slot() as usize;
            let next_slot = qstep.get_next_slot() as usize;
            *ctx.locals.get_mut(rows_slot).ok_or(RtError::MissingLocal)? = Some(page.rows);
            *ctx.locals.get_mut(next_slot).ok_or(RtError::MissingLocal)? =
                Some(Value::Str(page.next));
            if qstep.has_on_page() {
                let on_page = qstep.get_on_page().map_err(|_| RtError::Malformed)?;
                enqueue(ctx, on_page, queue)?;
            }
            Ok(true)
        }
        Err(code) => {
            let rows_slot = qstep.get_rows_slot() as usize;
            *ctx.locals.get_mut(rows_slot).ok_or(RtError::MissingLocal)? =
                Some(Value::Int(i64::from(code)));
            if qstep.has_on_err() {
                let on_err = qstep.get_on_err().map_err(|_| RtError::Malformed)?;
                enqueue(ctx, on_err, queue)?;
            }
            Ok(false)
        }
    }
}

fn enqueue(
    ctx: &mut EffectCtx<'_>,
    dispatch: ir::dispatch_step::Reader<'_>,
    queue: &mut Vec<Pending>,
) -> Result<(), RtError> {
    let payload_list = dispatch.get_payload().map_err(|_| RtError::Malformed)?;
    let mut payload = Vec::with_capacity(payload_list.len() as usize);
    for arg in payload_list.iter() {
        payload.push(eval_in(ctx, arg)?);
    }
    queue.push(Pending {
        event: dispatch.get_event(),
        case: dispatch.get_case(),
        payload,
        origin: Some(ctx.origin),
    });
    Ok(())
}

fn eval_in(ctx: &mut EffectCtx<'_>, expr: ir::expr::Reader<'_>) -> Result<Value, RtError> {
    let mut eval_ctx = EvalCtx {
        stores: ctx.stores,
        locals: ctx.locals,
        params: &[],
        device: ctx.device,
        locale: ctx.locale,
    };
    eval(&mut eval_ctx, expr)
}

fn symbol(symbols: &[alloc::string::String], id: u32) -> Result<&str, RtError> {
    symbols.get(id as usize).map(|s| s.as_str()).ok_or(RtError::Malformed)
}
