// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The semantics conformance corpus: `(state, event) → state'`
//! fixtures executed by the interpreter — and later re-executed by the AOT
//! tier (TASK-0079 parity gate). Any semantics change lands here first.
//! OWNERS: @ui @runtime
//! STATUS: Functional (growing every phase)
//! TEST_COVERAGE: this crate IS the coverage

// reason: test harness — a failed fixture step (parse/lower/mount/put) must
// panic loudly to fail the conformance test, not be silently propagated.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use nexus_dsl_runtime::{
    EffectHost, FixtureEnv, IdentityLocale, QueryCall, QueryPage, Runtime, Value,
};
use nexus_query::{Engine, MemKv, PageToken, QType, QVal, QuerySpec, Range, TableDef};

/// Compiles a `.nx` program to canonical `.nxir` bytes.
///
/// # Panics
/// On parse/check/lower failure — corpus programs must be valid.
#[must_use]
pub fn compile(source: &str) -> Vec<u8> {
    let file = nexus_dsl_core::parse_file(source).expect("corpus parses");
    let (model, diags) = nexus_dsl_core::check_file(&file);
    assert!(!nexus_dsl_core::has_errors(&diags), "corpus check errors: {diags:?}");
    let canonical = nexus_dsl_core::format_file(&file);
    nexus_dsl_core::lower_file(&file, &model, &canonical).expect("corpus lowers").nxir
}

/// A mounted program plus fixture environment — the corpus test harness.
pub struct Harness<'p> {
    pub runtime: Runtime<'p>,
    pub env: FixtureEnv,
    keys: Vec<u32>,
    symbols: Vec<String>,
}

impl<'p> Harness<'p> {
    /// # Panics
    /// If the payload does not mount.
    #[must_use]
    pub fn mount(nxir: &'p [u8]) -> Self {
        let runtime = Runtime::mount(nxir).expect("mounts");
        let symbols = runtime.symbols().to_vec();
        // i18n key table for the identity locale.
        let reader = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir).expect("reads");
        let keys: Vec<u32> = reader
            .root()
            .expect("root")
            .get_i18n_keys()
            .expect("keys")
            .iter()
            .map(|k| k.get_key())
            .collect();
        Self { runtime, env: FixtureEnv::default(), keys, symbols }
    }

    /// Dispatches an event by names with the given host.
    ///
    /// # Panics
    /// On unknown event/case or a runtime error — corpus dispatches must run.
    pub fn dispatch(
        &mut self,
        host: &mut dyn EffectHost,
        event: &str,
        case: &str,
        payload: Vec<Value>,
    ) {
        let (e, c) = self
            .runtime
            .event_case(event, case)
            .unwrap_or_else(|| panic!("unknown case {event}::{case}"));
        let locale = IdentityLocale { symbols: &self.symbols, keys: &self.keys };
        self.runtime
            .dispatch(&self.env, &locale, host, e, c, payload)
            .unwrap_or_else(|err| panic!("dispatch {event}::{case} failed: {err:?}"));
    }

    /// Asserts a store field's value.
    ///
    /// # Panics
    /// If the field is missing or differs.
    pub fn assert_field(&self, store: &str, field: &str, expected: &Value) {
        let actual = self
            .runtime
            .field(store, field)
            .unwrap_or_else(|| panic!("missing field {store}.{field}"));
        assert_eq!(actual, expected, "{store}.{field}");
    }
}

/// A scripted effect host: `(service.method, response)` pairs, in call order.
pub struct Script {
    pub responses: Vec<(&'static str, Result<Value, u32>)>,
    pub calls: Vec<String>,
    next: usize,
}

impl Script {
    #[must_use]
    pub fn new(responses: Vec<(&'static str, Result<Value, u32>)>) -> Self {
        Self { responses, calls: Vec::new(), next: 0 }
    }
}

impl EffectHost for Script {
    fn call(
        &mut self,
        service: &str,
        method: &str,
        _args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        let name = format!("{service}.{method}");
        self.calls.push(name.clone());
        let (expected, response) =
            self.responses.get(self.next).unwrap_or_else(|| panic!("unexpected call {name}"));
        assert_eq!(*expected, name, "call order");
        self.next += 1;
        response.clone()
    }
}

/// Stable error the [`EngineHost`] returns for an unknown query source.
pub const ERR_UNKNOWN_SOURCE: u32 = 1;

/// A query host backed by the REAL platform engine (`nexus-query` over an
/// in-memory KV) — the masterdetail consumer proof runs the same engine
/// queryd hosts. v1 fixture shape: each source is one table with a single
/// `name: Str` primary-key column; rows surface as `Str` values.
pub struct EngineHost {
    engine: Engine,
    kv: MemKv,
    sources: Vec<String>,
    /// Every query call, for order/shape assertions.
    pub queries: Vec<QueryCall>,
}

impl EngineHost {
    /// Builds sources from `(name, rows)` fixtures.
    ///
    /// # Panics
    /// On engine schema/typing errors — fixtures must be valid.
    #[must_use]
    pub fn new(sources: &[(&str, &[&str])]) -> Self {
        let mut tables = Vec::new();
        let mut names = Vec::new();
        for (i, (name, _)) in sources.iter().enumerate() {
            names.push(String::from(*name));
            tables.push(TableDef {
                id: i as u16,
                columns: vec![QType::Str],
                pk_col: 0,
                indexed: vec![],
            });
        }
        let engine = Engine::new(tables);
        let mut kv = MemKv::new();
        for (i, (_, rows)) in sources.iter().enumerate() {
            for row in *rows {
                engine
                    .put(&mut kv, i as u16, &[QVal::Str(String::from(*row))])
                    .expect("fixture row");
            }
        }
        Self { engine, kv, sources: names, queries: Vec::new() }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len() / 2).map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()).collect()
}

fn to_qval(value: &Value) -> Option<QVal> {
    match value {
        Value::Bool(b) => Some(QVal::Bool(*b)),
        Value::Int(i) => Some(QVal::Int(*i)),
        Value::Fx(f) => Some(QVal::Fx(*f)),
        Value::Str(s) => Some(QVal::Str(s.clone())),
        _ => None,
    }
}

impl EffectHost for EngineHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value], _: u32) -> Result<Value, u32> {
        Err(u32::MAX)
    }

    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        self.queries.push(call.clone());
        let table =
            self.sources.iter().position(|s| *s == call.source).ok_or(ERR_UNKNOWN_SOURCE)? as u16;
        // Single-column fixture: every column name maps to column 0.
        let mut spec = QuerySpec {
            table,
            eq: call
                .eq
                .iter()
                .map(|(_, v)| Some((0usize, to_qval(v)?)))
                .collect::<Option<Vec<_>>>()
                .ok_or(4u32)?,
            range: None,
            order_col: 0,
            descending: call.descending,
            limit: call.limit,
        };
        if call.low.is_some() || call.high.is_some() {
            spec.range = Some(Range {
                low: call.low.as_ref().and_then(to_qval),
                high: call.high.as_ref().and_then(to_qval),
            });
        }
        let token = if call.token.is_empty() {
            None
        } else {
            Some(hex_decode(&call.token).and_then(|b| PageToken::from_bytes(&b)).ok_or(6u32)?)
        };
        let page = self.engine.query(&self.kv, &spec, token.as_ref()).map_err(|_| 7u32)?;
        let rows = Value::List(
            page.rows
                .iter()
                .map(|row| match &row[0] {
                    QVal::Str(s) => Value::Str(s.clone()),
                    QVal::Int(i) => Value::Int(*i),
                    QVal::Fx(f) => Value::Fx(*f),
                    QVal::Bool(b) => Value::Bool(*b),
                })
                .collect(),
        );
        let next = page.next.map(|t| hex_encode(t.as_bytes())).unwrap_or_default();
        Ok(QueryPage { rows, next })
    }
}
