// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: windowd compositor runtime — the DSL `EffectHost` (TASK-0080C /
//! P1.3). The in-compositor DSL mount (greeter/shell) runs its `svc.*` effects
//! through THIS host instead of `NoIo`: it adapts the interpreter's service
//! seam to the routes windowd already holds — `bundlemgr.enumerate` →
//! [`crate::registry_client`], `ability.launch` → the compositor's launch path
//! (recorded here, executed by the runtime after dispatch to avoid borrowing
//! the mounted `View`), and `session.*` → [`crate::session_client`]. Every call
//! emits an honest marker with its result count, so the effect chain (page
//! `on Mount` → `dispatch` → `@effect` → service) is boot-verifiable.
//! OWNERS: @ui @runtime
//! STATUS: Experimental (TASK-0080C P1.3)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: the `svc.*` codecs are host-tested in their clients; this
//! adapter is proven via QEMU markers (`windowd: dsl svc …`).

use alloc::string::String;
use alloc::vec::Vec;
use nexus_dsl_runtime::{EffectHost, Value};

/// Stable effect error codes (surfaced to the DSL `Err(e)` arm).
const ERR_SVC_UNAVAILABLE: u32 = 1;
const ERR_SVC_UNKNOWN: u32 = 2;
const ERR_SVC_SHAPE: u32 = 3;

/// The DSL service host for the in-compositor mount. Holds the field symbol
/// ids the record shapes need (resolved once from the program's symbol table)
/// and a queue of `ability.launch` intents drained by the runtime AFTER
/// dispatch returns — `launch_app` needs `&mut DisplayServerRuntime`, which
/// the mounted `View` (owned by the runtime) is borrowing during the effect.
pub(crate) struct DslEffectHost {
    id_sym: Option<u32>,
    label_sym: Option<u32>,
    /// App ids requested via `svc.ability.launch`, in call order.
    pub launch_requests: Vec<String>,
}

impl DslEffectHost {
    pub(crate) fn new(symbols: &[String]) -> Self {
        Self {
            id_sym: symbols.iter().position(|s| s == "id").map(|i| i as u32),
            label_sym: symbols.iter().position(|s| s == "label").map(|i| i as u32),
            launch_requests: Vec::new(),
        }
    }

    /// `svc.bundlemgr.enumerate(query)` → `List<AppEntry{ id, label }>` from the
    /// installed-bundle registry. Query filtering is service-side (RFC-0065);
    /// v1 returns the full list. Records are FIELD-SORTED by symbol id (the
    /// `Value::Record` contract).
    fn enumerate(&self) -> Result<Value, u32> {
        #[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
        {
            let (Some(id_sym), Some(label_sym)) = (self.id_sym, self.label_sym) else {
                let _ = nexus_abi::debug_println(
                    "windowd: dsl svc bundlemgr.enumerate FAIL (no id/label symbol)",
                );
                return Err(ERR_SVC_SHAPE);
            };
            match crate::registry_client::fetch_app_menu() {
                Some(menu) => {
                    let rows: Vec<Value> = menu
                        .entries()
                        .iter()
                        .map(|e| {
                            let mut fields = alloc::vec![
                                (id_sym, Value::Str(e.id.clone())),
                                (label_sym, Value::Str(e.label.clone())),
                            ];
                            fields.sort_by_key(|(sym, _)| *sym);
                            Value::Record(fields)
                        })
                        .collect();
                    let _ = nexus_abi::debug_println(&alloc::format!(
                        "windowd: dsl svc bundlemgr.enumerate ok (n={})",
                        rows.len()
                    ));
                    Ok(Value::List(rows))
                }
                None => {
                    let _ = nexus_abi::debug_println(
                        "windowd: dsl svc bundlemgr.enumerate FAIL (registry unreachable)",
                    );
                    Err(ERR_SVC_UNAVAILABLE)
                }
            }
        }
        #[cfg(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
        {
            let _ = (self.id_sym, self.label_sym);
            Err(ERR_SVC_UNAVAILABLE)
        }
    }

    /// `svc.session.users()` → `List<Str>` of the greeter user display names.
    fn session_users(&self) -> Result<Value, u32> {
        #[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
        {
            match crate::session_client::fetch_session_state() {
                Some(snapshot) => {
                    let rows: Vec<Value> = snapshot
                        .users
                        .iter()
                        .map(|u| Value::Str(u.display_name.clone()))
                        .collect();
                    let _ = nexus_abi::debug_println(&alloc::format!(
                        "windowd: dsl svc session.users ok (n={})",
                        rows.len()
                    ));
                    Ok(Value::List(rows))
                }
                None => {
                    let _ = nexus_abi::debug_println(
                        "windowd: dsl svc session.users FAIL (sessiond unreachable)",
                    );
                    Err(ERR_SVC_UNAVAILABLE)
                }
            }
        }
        #[cfg(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
        {
            Err(ERR_SVC_UNAVAILABLE)
        }
    }
}

fn str_of(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

impl EffectHost for DslEffectHost {
    fn call(
        &mut self,
        service: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        match (service, method) {
            ("bundlemgr", "enumerate") => self.enumerate(),
            ("session", "users") => self.session_users(),
            ("ability", "launch") => {
                let id = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                // Record the intent; the runtime drains it after dispatch and
                // drives the real RFC-0065 launch (abilitymgr is the authority).
                #[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
                let _ = nexus_abi::debug_println(&alloc::format!(
                    "windowd: dsl svc ability.launch({id})"
                ));
                self.launch_requests.push(String::from(id));
                Ok(Value::Bool(true))
            }
            ("session", "login") => {
                let user = args.first().and_then(str_of).ok_or(ERR_SVC_SHAPE)?;
                #[cfg(all(feature = "os-lite", nexus_env = "os", target_os = "none"))]
                {
                    match crate::session_client::login(user) {
                        Some(_) => Ok(Value::Bool(true)),
                        None => Ok(Value::Bool(false)),
                    }
                }
                #[cfg(not(all(feature = "os-lite", nexus_env = "os", target_os = "none")))]
                {
                    let _ = user;
                    Err(ERR_SVC_UNAVAILABLE)
                }
            }
            _ => Err(ERR_SVC_UNKNOWN),
        }
    }
}
