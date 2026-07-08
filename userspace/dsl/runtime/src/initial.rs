// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Initial-load effects, derived from the dataflow (no lifecycle hook).
//!
//! principles.md §5 forbids a second effect-trigger model — there is no
//! `on Mount`/`useEffect` in the language. The initial load falls out of the
//! dataflow instead: an event that carries an `@effect` but is dispatched by
//! NOTHING — no handler, no reducer, no other effect — is a ROOT. It can only
//! ever run once, at mount, so the runtime runs it. Writing the obvious
//! program (`@effect on Load { … }` with nothing dispatching `Load`) just
//! loads; there is no lifecycle code to write and none to get wrong.
//!
//! This is pure static analysis over the IR — deterministic and bounded — run
//! once at [`View::mount`](crate::View::mount).

use crate::RtError;
use alloc::vec::Vec;
use nexus_dsl_ir::ui_ir_capnp as ir;

/// Every ROOT effect-event `(event, case)`: an effect trigger that no dispatch
/// step (in any effect) and no handler (in any component's view) targets.
/// Declaration order is preserved, so the initial-load firing order is
/// deterministic.
pub(crate) fn root_effect_events(
    root: ir::ui_program::Reader<'_>,
) -> Result<Vec<(u32, u32)>, RtError> {
    let effects = root.get_effects().map_err(|_| RtError::Malformed)?;

    // Candidate roots: every effect's trigger event, in declaration order.
    let mut triggers: Vec<(u32, u32)> = Vec::with_capacity(effects.len() as usize);
    for e in effects.iter() {
        triggers.push((e.get_event(), e.get_case()));
    }

    // Anything dispatched somewhere is NOT a root (it has an explicit driver).
    let mut dispatched: Vec<(u32, u32)> = Vec::new();

    // (a) dispatches inside effect plans (onOk / onErr / dispatch / onPage).
    for e in effects.iter() {
        for step in e.get_steps().map_err(|_| RtError::Malformed)?.iter() {
            use ir::effect_step::Which;
            match step.which().map_err(|_| RtError::Malformed)? {
                Which::Call(c) => {
                    let c = c.map_err(|_| RtError::Malformed)?;
                    if c.has_on_ok() {
                        push_dispatch(c.get_on_ok(), &mut dispatched)?;
                    }
                    if c.has_on_err() {
                        push_dispatch(c.get_on_err(), &mut dispatched)?;
                    }
                }
                Which::Dispatch(d) => push_dispatch(d, &mut dispatched)?,
                Which::Query(q) => {
                    let q = q.map_err(|_| RtError::Malformed)?;
                    if q.has_on_page() {
                        push_dispatch(q.get_on_page(), &mut dispatched)?;
                    }
                    if q.has_on_err() {
                        push_dispatch(q.get_on_err(), &mut dispatched)?;
                    }
                }
            }
        }
    }

    // (b) dispatches from handlers, across every component's view tree.
    for comp in root.get_components().map_err(|_| RtError::Malformed)?.iter() {
        let view = comp.get_view().map_err(|_| RtError::Malformed)?;
        collect_handler_dispatches(view, &mut dispatched)?;
    }

    Ok(triggers.into_iter().filter(|t| !dispatched.contains(t)).collect())
}

fn push_dispatch(
    step: Result<ir::dispatch_step::Reader<'_>, capnp::Error>,
    out: &mut Vec<(u32, u32)>,
) -> Result<(), RtError> {
    let d = step.map_err(|_| RtError::Malformed)?;
    out.push((d.get_event(), d.get_case()));
    Ok(())
}

fn collect_handler_dispatches(
    node: ir::view_node::Reader<'_>,
    out: &mut Vec<(u32, u32)>,
) -> Result<(), RtError> {
    use ir::view_node::Which;
    match node.which().map_err(|_| RtError::Malformed)? {
        Which::Widget(w) => {
            let w = w.map_err(|_| RtError::Malformed)?;
            for h in w.get_handlers().map_err(|_| RtError::Malformed)?.iter() {
                if let Ok(ir::handler::Which::Dispatch(Ok(d))) = h.which() {
                    out.push((d.get_event(), d.get_case()));
                }
            }
            for c in w.get_children().map_err(|_| RtError::Malformed)?.iter() {
                collect_handler_dispatches(c, out)?;
            }
        }
        Which::ForEach(f) => {
            let f = f.map_err(|_| RtError::Malformed)?;
            let template = f.get_template().map_err(|_| RtError::Malformed)?;
            collect_handler_dispatches(template, out)?;
        }
        Which::Branch(b) => {
            let b = b.map_err(|_| RtError::Malformed)?;
            for arm in b.get_arms().map_err(|_| RtError::Malformed)?.iter() {
                for n in arm.get_body().map_err(|_| RtError::Malformed)?.iter() {
                    collect_handler_dispatches(n, out)?;
                }
            }
            for n in b.get_else_body().map_err(|_| RtError::Malformed)?.iter() {
                collect_handler_dispatches(n, out)?;
            }
        }
        // A component ref's own view is walked when we iterate all components.
        Which::ComponentRef(_) => {}
    }
    Ok(())
}
