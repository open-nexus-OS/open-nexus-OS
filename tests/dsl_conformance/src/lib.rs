// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The semantics conformance corpus: `(state, event) → state'`
//! fixtures executed by the interpreter — and later re-executed by the AOT
//! tier (TASK-0079 parity gate). Any semantics change lands here first.
//! OWNERS: @ui @runtime
//! STATUS: Functional (growing every phase)
//! TEST_COVERAGE: this crate IS the coverage

use nexus_dsl_runtime::{
    EffectHost, FixtureEnv, IdentityLocale, Runtime, Value,
};

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
        let reader = nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir)
            .expect("reads");
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
    pub fn dispatch(&mut self, host: &mut dyn EffectHost, event: &str, case: &str, payload: Vec<Value>) {
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
        let (expected, response) = self
            .responses
            .get(self.next)
            .unwrap_or_else(|| panic!("unexpected call {name}"));
        assert_eq!(*expected, name, "call order");
        self.next += 1;
        response.clone()
    }
}
