// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

//! CONTEXT: `nexus-dsl-runtime` — the `.nxir` interpreter: mounts a validated
//! program, owns store state, executes reducers (pure) and effect plans (IO
//! via injected [`EffectHost`]), and drives the dispatch queue. One semantics
//! carrier for three hosts: host harness, in-compositor mount, app-host
//! process (docs/dev/dsl/runtime.md).
//! OWNERS: @ui @runtime
//! STATUS: In progress (TASK-0076)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: unit tests + `tests/dsl_conformance` + `tests/dsl_goldens`
//! DOCS: docs/dev/dsl/{state,runtime,ir}.md

extern crate alloc;

pub mod anim;
pub mod effects;
pub mod emit;
pub mod i18n;
mod initial;
pub mod interact;
pub mod nav;
pub mod persist;
pub mod reduce;
pub mod registry;
pub mod store;
pub mod svc;
pub mod view;

pub use anim::{
    AnimIntent, AnimKind, LOOP_BREATHE, LOOP_CAROUSEL, LOOP_CAROUSEL_SPOKES, LOOP_SWEEP,
};
pub use emit::{Damage, Dep};
pub use i18n::{Catalog, LocaleChain};
pub use interact::HandlerEntry;
pub use nav::{Nav, NavEntry};
pub use nexus_theme_tokens as theme_tokens;
pub use store::{StoreState, Value};
pub use view::View;

use alloc::{collections::BTreeMap, string::String, vec, vec::Vec};
use effects::Pending;
use nexus_dsl_ir::read::ProgramReader;
use nexus_dsl_ir::IrError;

/// Deterministic runtime errors (stable, matchable — never formatted away).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtError {
    /// IR reading failed mid-walk (validated programs never hit this).
    Malformed,
    TypeMismatch,
    UnknownField,
    MissingLocal,
    Overflow,
    DivByZero,
    /// Construct not yet executable at this phase (e.g. query steps).
    Unsupported,
    /// A bound was exceeded (dispatch cascade, queue length).
    Budget,
    /// Two collection items evaluated to the same `.key(expr)` value.
    DuplicateKey,
}

/// Read-only device environment (fed from the shell-config registry on OS,
/// from fixtures on the host). Field ids index `registry::DEVICE_FIELDS`.
pub trait DeviceEnv {
    fn get(&self, field_id: u32) -> Value;
}

/// Locale/i18n source. `key` indexes the program's `i18nKeys` table.
pub trait LocaleSource {
    fn format(&self, key: u32, args: &[Value]) -> String;
}

/// Stable error code for hosts without query support (the default impl).
pub const ERR_QUERY_UNSUPPORTED: u32 = u32::MAX - 1;

/// A fully resolved query execution request (the `query` effect step):
/// symbols textualized, predicate values evaluated, the v1 shape flattened
/// into eq-conjunction + inclusive bounds on the order column
/// (docs/dev/dsl/db-queries.md). The host maps this onto `nexus-query`
/// (fixtures) or the queryd wire schema (app-host) — the runtime itself
/// never touches storage.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryCall {
    pub source: String,
    /// Equality predicates (column name, value).
    pub eq: Vec<(String, Value)>,
    /// Inclusive bounds on the ORDER column (v1 rule).
    pub low: Option<Value>,
    pub high: Option<Value>,
    pub order_col: String,
    pub descending: bool,
    pub limit: u32,
    /// Opaque page token (hex; `""` = first page).
    pub token: String,
}

/// One page: rows as a DSL list value + the continuation token
/// (`""` = exhausted).
#[derive(Debug, Clone, PartialEq)]
pub struct QueryPage {
    pub rows: Value,
    pub next: String,
}

/// The IO boundary: effects call services only through this.
pub trait EffectHost {
    /// Returns the call result or a stable error code.
    fn call(
        &mut self,
        service: &str,
        method: &str,
        args: &[Value],
        timeout_ms: u32,
    ) -> Result<Value, u32>;

    /// Executes a query step. Hosts without a query engine keep the default
    /// (a deterministic error — never a silent empty page).
    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        let _ = call;
        Err(ERR_QUERY_UNSUPPORTED)
    }
}

/// Fixture environment: the full read-only device contract
/// (docs/dev/dsl/profiles.md), host-injectable per golden variant. Field ids
/// index `nexus-dsl-core::registry::DEVICE_FIELDS`:
/// 0 profile, 1 posture, 2 orientation, 3 shellMode, 4 sizeClass,
/// 5 dpiClass, 6 input.
pub struct FixtureEnv {
    pub profile: &'static str,
    pub posture: &'static str,
    pub orientation: &'static str,
    pub shell_mode: &'static str,
    pub size_class: &'static str,
    pub dpi_class: &'static str,
    /// Input capability names present on this device (`touch`, `mouse`, …).
    pub input: &'static [&'static str],
}

impl Default for FixtureEnv {
    fn default() -> Self {
        Self::desktop()
    }
}

impl FixtureEnv {
    #[must_use]
    pub fn desktop() -> Self {
        Self {
            profile: "desktop",
            posture: "",
            orientation: "landscape",
            shell_mode: "desktop",
            size_class: "wide",
            dpi_class: "normal",
            input: &["mouse", "kbd", "touch"],
        }
    }

    #[must_use]
    pub fn phone(orientation: &'static str) -> Self {
        Self {
            profile: "phone",
            posture: "",
            orientation,
            shell_mode: "phone",
            size_class: "compact",
            dpi_class: "high",
            input: &["touch"],
        }
    }

    #[must_use]
    pub fn tablet(orientation: &'static str) -> Self {
        Self {
            profile: "tablet",
            posture: "",
            orientation,
            shell_mode: "tablet",
            // The touch width classes (design_handoff_launcher): landscape
            // (≥1024) = wide, portrait = regular. Hosts override from the
            // REAL surface width; this preset mirrors that mapping.
            size_class: if matches!(orientation, "landscape") { "wide" } else { "regular" },
            dpi_class: "high",
            input: &["touch", "kbd"],
        }
    }

    /// A convertible in an explicit shell mode (`desktop` or `tablet`).
    #[must_use]
    pub fn convertible(shell_mode: &'static str) -> Self {
        Self {
            profile: "convertible",
            posture: "",
            orientation: "landscape",
            shell_mode,
            size_class: "regular",
            dpi_class: "normal",
            input: &["touch", "mouse", "kbd"],
        }
    }
}

impl DeviceEnv for FixtureEnv {
    fn get(&self, field_id: u32) -> Value {
        match field_id {
            0 => Value::Str(String::from(self.profile)),
            1 => Value::Str(String::from(self.posture)),
            2 => Value::Str(String::from(self.orientation)),
            3 => Value::Str(String::from(self.shell_mode)),
            4 => Value::Str(String::from(self.size_class)),
            5 => Value::Str(String::from(self.dpi_class)),
            6 => {
                Value::List(self.input.iter().map(|name| Value::Str(String::from(*name))).collect())
            }
            _ => Value::Str(String::new()),
        }
    }
}

/// A host that fails every call — for programs whose effects must not run.
pub struct NoIo;

impl EffectHost for NoIo {
    fn call(&mut self, _: &str, _: &str, _: &[Value], _: u32) -> Result<Value, u32> {
        Err(u32::MAX)
    }
}

/// Cap on dispatch cascades (event → effect → dispatch → …).
pub const MAX_DISPATCH_CASCADE: usize = 64;

/// A changed store field, reported after each dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChangedField {
    pub store: u32,
    pub field: u32,
}

/// The mounted program.
pub struct Runtime<'p> {
    reader: ProgramReader<'p>,
    symbols: Vec<String>,
    stores: Vec<StoreState>,
    locals: Vec<Option<Value>>,
    /// Reusable change accumulator (drained by `dispatch`).
    changed: Vec<ChangedField>,
    /// Cancellation generations per (event, case): re-triggering a case
    /// bumps its generation; pending follow-ups from an older generation of
    /// that trigger are dropped when dequeued (latest wins — the async
    /// recipe contract, docs/dev/dsl/state.md).
    generations: BTreeMap<(u32, u32), u32>,
}

impl<'p> Runtime<'p> {
    /// Validates and mounts a canonical `.nxir` payload (fail-closed).
    ///
    /// # Errors
    /// Any [`IrError`] from reading/validation, [`RtError`] from default
    /// evaluation.
    pub fn mount(bytes: &'p [u8]) -> Result<Self, MountError> {
        let reader = ProgramReader::from_canonical_bytes(bytes).map_err(MountError::Ir)?;
        {
            let root = reader.root().map_err(MountError::Ir)?;
            nexus_dsl_ir::validate::validate_program(root).map_err(MountError::Ir)?;
        }

        let root = reader.root().map_err(MountError::Ir)?;
        let mut symbols = Vec::new();
        for symbol in root.get_symbols().map_err(|_| MountError::Ir(IrError::Malformed))?.iter() {
            symbols.push(String::from(
                symbol
                    .map_err(|_| MountError::Ir(IrError::Malformed))?
                    .to_str()
                    .map_err(|_| MountError::Ir(IrError::Malformed))?,
            ));
        }
        let max_locals = root.get_budgets().map(|b| b.get_max_locals()).unwrap_or(32) as usize;

        // Store state from defaults (constant expressions).
        let mut stores = Vec::new();
        let device = FixtureEnv::default();
        let locale = IdentityLocale { symbols: &symbols, keys: &[] };
        for store in root.get_stores().map_err(|_| MountError::Ir(IrError::Malformed))?.iter() {
            let field_list = store.get_fields().map_err(|_| MountError::Ir(IrError::Malformed))?;
            let mut fields = Vec::with_capacity(field_list.len() as usize);
            let mut field_syms = Vec::with_capacity(field_list.len() as usize);
            let mut locals: Vec<Option<Value>> = vec![None; max_locals];
            for field in field_list.iter() {
                field_syms.push(field.get_name());
                let value = if field.has_default() {
                    let mut ctx = reduce::EvalCtx {
                        stores: &stores,
                        locals: &mut locals,
                        params: &[],
                        device: &device,
                        locale: &locale,
                    };
                    reduce::eval(
                        &mut ctx,
                        field.get_default().map_err(|_| MountError::Ir(IrError::Malformed))?,
                    )
                    .map_err(MountError::Rt)?
                } else {
                    Value::zero_of(
                        field.get_type().map_err(|_| MountError::Ir(IrError::Malformed))?,
                    )
                };
                fields.push(value);
            }
            stores.push(StoreState::new(fields, field_syms));
        }

        Ok(Self {
            reader,
            symbols,
            stores,
            locals: vec![None; max_locals],
            changed: Vec::new(),
            generations: BTreeMap::new(),
        })
    }

    #[must_use]
    pub fn symbols(&self) -> &[String] {
        &self.symbols
    }

    /// The underlying bounded program reader.
    #[must_use]
    pub fn reader(&self) -> &ProgramReader<'p> {
        &self.reader
    }

    #[must_use]
    pub fn stores(&self) -> &[StoreState] {
        &self.stores
    }

    /// Mutable store access for the `@persist` restore path (crate-internal;
    /// hosts restore through [`Runtime::persist_restore`]).
    pub(crate) fn stores_mut(&mut self) -> &mut [StoreState] {
        &mut self.stores
    }

    /// Snapshot every store's live field values, keyed by field SYMBOL.
    /// Host-side state preservation across a REMOUNT of the same program
    /// (live re-theme): a remount resets stores to their declared defaults,
    /// which silently closed overlays and reset sliders mid-session.
    #[must_use]
    pub fn store_snapshot(&self) -> Vec<Vec<(u32, Value)>> {
        self.stores
            .iter()
            .map(|s| s.field_syms.iter().copied().zip(s.fields.iter().cloned()).collect())
            .collect()
    }

    /// Restore a [`Runtime::store_snapshot`]. Defensive by construction:
    /// stores match by INDEX (same program → same order) but fields match by
    /// SYMBOL, so a snapshot from a different program shape restores nothing
    /// it cannot prove — it never misassigns a value to the wrong field.
    pub fn store_restore(&mut self, snapshot: &[Vec<(u32, Value)>]) {
        for (state, snap) in self.stores.iter_mut().zip(snapshot.iter()) {
            for &(sym, ref value) in snap {
                if let Ok(index) = state.field_index(sym) {
                    if let Some(slot) = state.fields.get_mut(index) {
                        *slot = value.clone();
                    }
                }
            }
        }
    }

    /// Two-way binding write: the ONE store mutation machinery reducers use
    /// (compare-and-mark via `set_path`), driven by an interaction value.
    /// Returns the changed fields like `dispatch`.
    ///
    /// # Errors
    /// Unknown store/field or a type-mismatched write target.
    pub fn write_binding(
        &mut self,
        store: u32,
        path: &[u32],
        value: Value,
    ) -> Result<Vec<ChangedField>, RtError> {
        self.changed.clear();
        let state = self.stores.get_mut(store as usize).ok_or(RtError::UnknownField)?;
        state.set_path(path, value)?;
        let changed = &mut self.changed;
        state.take_changes(|field| {
            changed.push(ChangedField { store, field: field as u32 });
        });
        Ok(core::mem::take(&mut self.changed))
    }

    /// Reads a bound field's current value (for flip-style interactions).
    #[must_use]
    pub fn read_binding(&self, store: u32, path: &[u32]) -> Option<&Value> {
        let state = self.stores.get(store as usize)?;
        let index = state.field_index(*path.first()?).ok()?;
        state.get(index).ok()
    }

    /// Reads a store field by names (test/debug convenience).
    pub fn field(&self, store_name: &str, field_name: &str) -> Option<&Value> {
        let root = self.reader.root().ok()?;
        let store_sym = self.symbols.iter().position(|s| s == store_name)? as u32;
        let field_sym = self.symbols.iter().position(|s| s == field_name)? as u32;
        let stores = root.get_stores().ok()?;
        for (i, store) in stores.iter().enumerate() {
            if store.get_name() == store_sym {
                let state = self.stores.get(i)?;
                let index = state.field_index(field_sym).ok()?;
                return state.get(index).ok();
            }
        }
        None
    }

    /// Resolves an event case by names (test/host convenience).
    pub fn event_case(&self, event: &str, case: &str) -> Option<(u32, u32)> {
        let root = self.reader.root().ok()?;
        let event_sym = self.symbols.iter().position(|s| s == event)? as u32;
        let case_sym = self.symbols.iter().position(|s| s == case)? as u32;
        for (e, decl) in root.get_events().ok()?.iter().enumerate() {
            if decl.get_name() == event_sym {
                for (c, case_decl) in decl.get_cases().ok()?.iter().enumerate() {
                    if case_decl.get_name() == case_sym {
                        return Some((e as u32, c as u32));
                    }
                }
            }
        }
        None
    }

    /// Dispatches one event: reduce (pure, commit) → effects (queued
    /// follow-ups) → drain the queue, bounded by [`MAX_DISPATCH_CASCADE`].
    /// Returns the changed fields (deduplicated, deterministic order).
    ///
    /// # Errors
    /// Deterministic [`RtError`]s; state remains the last committed snapshot
    /// of every completed reduce (a failing cascade stops, never tears).
    pub fn dispatch(
        &mut self,
        env: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        event: u32,
        case: u32,
        payload: Vec<Value>,
    ) -> Result<Vec<ChangedField>, RtError> {
        self.changed.clear();
        // FIFO: follow-up dispatches run in the order effects produced them
        // (a LIFO Vec::pop would reverse sibling dispatches — observed-order
        // determinism is part of the written semantics).
        let mut queue: Vec<Pending> = vec![Pending { event, case, payload, origin: None }];
        let mut steps = 0usize;
        while !queue.is_empty() {
            let pending = queue.remove(0);
            steps += 1;
            if steps > MAX_DISPATCH_CASCADE {
                return Err(RtError::Budget);
            }
            // Cancellation: a follow-up whose originating trigger has been
            // re-fired since it was enqueued is stale — latest wins.
            if let Some((oe, oc, ogen)) = pending.origin {
                if self.generations.get(&(oe, oc)).copied().unwrap_or(0) != ogen {
                    continue;
                }
            }
            self.dispatch_one(env, locale, host, &pending, &mut queue)?;
        }
        // Deterministic order + dedup.
        self.changed.sort_by_key(|c| (c.store, c.field));
        self.changed.dedup();
        Ok(core::mem::take(&mut self.changed))
    }

    fn dispatch_one(
        &mut self,
        env: &dyn DeviceEnv,
        locale: &dyn LocaleSource,
        host: &mut dyn EffectHost,
        pending: &Pending,
        queue: &mut Vec<Pending>,
    ) -> Result<(), RtError> {
        let root = self.reader.root().map_err(|_| RtError::Malformed)?;

        // 1. Reduce (pure) + commit.
        for reducer in root.get_reducers().map_err(|_| RtError::Malformed)?.iter() {
            if reducer.get_event() != pending.event {
                continue;
            }
            for arm in reducer.get_arms().map_err(|_| RtError::Malformed)?.iter() {
                if arm.get_case() != pending.case {
                    continue;
                }
                self.locals.fill(None);
                let binds = arm.get_binds().map_err(|_| RtError::Malformed)?;
                for (i, value) in pending.payload.iter().enumerate() {
                    if i < binds.len() as usize {
                        let slot = binds.get(i as u32) as usize;
                        *self.locals.get_mut(slot).ok_or(RtError::MissingLocal)? =
                            Some(value.clone());
                    }
                }
                let store_index = reducer.get_store() as usize;
                let mut ctx = reduce::ExecCtx {
                    store_index,
                    stores: &mut self.stores,
                    locals: &mut self.locals,
                    params: &[],
                    device: env,
                    locale,
                };
                reduce::exec(&mut ctx, arm.get_body().map_err(|_| RtError::Malformed)?)?;
                let store_id = store_index as u32;
                let changed = &mut self.changed;
                if let Some(store) = self.stores.get_mut(store_index) {
                    store.take_changes(|field| {
                        changed.push(ChangedField { store: store_id, field: field as u32 });
                    });
                }
            }
        }

        // 2. Effects (post-commit; follow-ups enter the queue). Firing this
        // trigger bumps its generation — older in-flight follow-ups die.
        let generation = {
            let slot = self.generations.entry((pending.event, pending.case)).or_insert(0);
            *slot += 1;
            *slot
        };
        for plan in root.get_effects().map_err(|_| RtError::Malformed)?.iter() {
            if plan.get_event() != pending.event || plan.get_case() != pending.case {
                continue;
            }
            self.locals.fill(None);
            let mut ctx = effects::EffectCtx {
                origin: (pending.event, pending.case, generation),
                stores: &self.stores,
                locals: &mut self.locals,
                device: env,
                locale,
                host,
                symbols: &self.symbols,
                root,
            };
            effects::run_plan(&mut ctx, plan, &pending.payload, queue)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountError {
    Ir(IrError),
    Rt(RtError),
}

/// Identity locale: formats a key as its own text (pseudo-locale for tests;
/// real catalogs land in v0.2a).
pub struct IdentityLocale<'a> {
    pub symbols: &'a [String],
    /// i18n key table: key index → symbol id.
    pub keys: &'a [u32],
}

impl LocaleSource for IdentityLocale<'_> {
    fn format(&self, key: u32, _args: &[Value]) -> String {
        self.keys
            .get(key as usize)
            .and_then(|&sym| self.symbols.get(sym as usize))
            .cloned()
            .unwrap_or_default()
    }
}
