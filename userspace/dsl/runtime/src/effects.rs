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
use crate::{DeviceEnv, EffectHost, LocaleSource, RtError};
use alloc::vec::Vec;
use nexus_dsl_ir::ui_ir_capnp as ir;

/// A queued follow-up dispatch produced by an effect.
pub(crate) struct Pending {
    pub event: u32,
    pub case: u32,
    pub payload: Vec<Value>,
}

pub(crate) struct EffectCtx<'a> {
    pub stores: &'a [StoreState],
    pub locals: &'a mut [Option<Value>],
    pub device: &'a dyn DeviceEnv,
    pub locale: &'a dyn LocaleSource,
    pub host: &'a mut dyn EffectHost,
    pub symbols: &'a [alloc::string::String],
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
                            *ctx.locals
                                .get_mut(slot as usize)
                                .ok_or(RtError::MissingLocal)? = Some(result.clone());
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
            Which::Query(_) => return Err(RtError::Unsupported), // QuerySpec v1 task
        }
    }
    Ok(())
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
    queue.push(Pending { event: dispatch.get_event(), case: dispatch.get_case(), payload });
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

fn symbol<'a>(symbols: &'a [alloc::string::String], id: u32) -> Result<&'a str, RtError> {
    symbols.get(id as usize).map(|s| s.as_str()).ok_or(RtError::Malformed)
}
