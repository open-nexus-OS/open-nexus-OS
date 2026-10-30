// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: `svc.shortcuts.list(category, locale)` (RFC-0113 §7, TASK-0346 P3) — the shortcut
//! registry's rows for the settings page, in process (the registry is compiled into every
//! component that needs it; no service round trip). One `ShortcutRow { id, keys, alt, active }`
//! per entry of the category, in table order: `keys` names the primary chord, `alt` the
//! alternates (` · ` between them, "" for none), both for `locale`; `active` false = matched but
//! nothing acts on it yet. The page names a row from its language files by `id`. An unknown
//! category is an empty list — bounded, never an error popup.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: the registry's `tests/registry.rs` (ids ↔ the page's names in five languages);
//!   the page in `tests/dsl_apps_conformance/tests/settings.rs`; on the board the `shortcuts` act
//!   (`apphost: shortcuts listed (`, once per boot)

#![cfg(all(nexus_env = "os", target_arch = "riscv64", target_os = "none"))]

use crate::effect_host::{raw_marker, str_of, AppEffectHost, ERR_SVC_SHAPE, ERR_SVC_UNKNOWN};
use alloc::string::String;
use core::sync::atomic::{AtomicBool, Ordering};
use nexus_dsl_runtime::Value;

/// `apphost: shortcuts listed (` is said once per boot: the board's `shortcuts` act line.
static LISTED_SAID: AtomicBool = AtomicBool::new(false);

/// The row field symbols beyond the host's shared `id` — interned only when a page reads them.
pub(crate) struct ShortcutSyms {
    keys: Option<u32>,
    alt: Option<u32>,
    active: Option<u32>,
}

impl ShortcutSyms {
    pub(crate) fn new(symbols: &[String]) -> Self {
        let sym = |name: &str| symbols.iter().position(|s| s == name).map(|i| i as u32);
        Self { keys: sym("keys"), alt: sym("alt"), active: sym("active") }
    }
}

impl AppEffectHost {
    pub(crate) fn shortcuts_call(&self, method: &str, args: &[Value]) -> Result<Value, u32> {
        match method {
            "list" => {
                let category = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                self.shortcuts_list(category, args.get(1).and_then(str_of).unwrap_or(""))
            }
            _ => Err(ERR_SVC_UNKNOWN),
        }
    }

    fn shortcuts_list(&self, category: &str, locale: &str) -> Result<Value, u32> {
        let Some(id_sym) = self.id_sym else {
            return Err(ERR_SVC_SHAPE); // the page never reads a row
        };
        let syms = &self.shortcuts;
        let rows = shortcuts::listing(category, locale)
            .into_iter()
            .map(|row| {
                let mut fields = alloc::vec![(id_sym, Value::Str(String::from(row.id)))];
                if let Some(sym) = syms.keys {
                    fields.push((sym, Value::Str(row.keys)));
                }
                if let Some(sym) = syms.alt {
                    fields.push((sym, Value::Str(row.alt)));
                }
                if let Some(sym) = syms.active {
                    fields.push((sym, Value::Bool(row.active)));
                }
                fields.sort_by_key(|(sym, _)| *sym);
                Value::Record(fields)
            })
            .collect::<alloc::vec::Vec<_>>();
        if !rows.is_empty() && !LISTED_SAID.swap(true, Ordering::Relaxed) {
            raw_marker(&alloc::format!("apphost: shortcuts listed (rows={})", rows.len()));
        }
        Ok(Value::List(rows))
    }
}
