// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A pipeline's overrides (WGSL's pipeline-overridable constants), fixed when it is built: the
//! values come keyed as a pipeline's constants are — an override's `@id` in decimal, else its
//! name — and an override not given keeps its default. These are the library's variants of one
//! source (ADR-0072 §5).

use std::collections::HashMap;

use naga::{Expression, Handle, Literal, ScalarKind, TypeInner};

use super::FrontendError;

/// Every override's value for this pipeline: the one given under its key, else its default (a
/// literal). A key naming no override or given twice, a value its type cannot hold, an override
/// with neither are refused.
pub(crate) fn fix(
    module: &naga::Module,
    given: &[(&str, f64)],
) -> Result<HashMap<Handle<naga::Override>, Literal>, FrontendError> {
    let refuse = |what: String| Err(FrontendError::Override(what));
    for (i, (key, _)) in given.iter().enumerate() {
        if given[..i].iter().any(|(k, _)| k == key) {
            return refuse(format!("{key} given twice"));
        }
    }
    let mut used = vec![false; given.len()];
    let mut fixed = HashMap::new();
    for (h, o) in module.overrides.iter() {
        let key = match (o.id, &o.name) {
            (Some(id), _) => id.to_string(),
            (None, Some(name)) => name.clone(),
            (None, None) => return refuse("an override without a name".into()),
        };
        let lit = match (given.iter().position(|(k, _)| *k == key), o.init) {
            (Some(i), _) => {
                used[i] = true;
                let x = given[i].1;
                let TypeInner::Scalar(s) = module.types[o.ty].inner else {
                    return refuse(format!("{key} is not a scalar"));
                };
                let whole = x.fract() == 0.0;
                match (s.kind, s.width) {
                    (ScalarKind::Bool, _) if !x.is_nan() => Literal::Bool(x != 0.0),
                    (ScalarKind::Float, 4) if (x as f32).is_finite() => Literal::F32(x as f32),
                    (ScalarKind::Sint, 4)
                        if whole && (-2_147_483_648.0..2_147_483_648.0).contains(&x) =>
                    {
                        Literal::I32(x as i32)
                    }
                    (ScalarKind::Uint, 4) if whole && (0.0..4_294_967_296.0).contains(&x) => {
                        Literal::U32(x as u32)
                    }
                    _ => return refuse(format!("{key}={x} outside its type")),
                }
            }
            (None, Some(init)) => match module.global_expressions[init] {
                Expression::Literal(lit) => lit,
                _ => return refuse(format!("{key}'s default is not a literal")),
            },
            (None, None) => return refuse(format!("{key} has no value")),
        };
        fixed.insert(h, lit);
    }
    match used.iter().position(|u| !u) {
        Some(i) => refuse(format!("no override {}", given[i].0)),
        None => Ok(fixed),
    }
}
