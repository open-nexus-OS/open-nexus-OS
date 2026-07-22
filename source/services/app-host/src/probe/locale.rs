// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: app-host locale plumbing (RFC-0077) — splits the bundle payload
//! into NXIR + locale-pack catalogs and defines the `app_locale!` source
//! (active catalog over the baked default) used at every dispatch site.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Internal
//! TEST_COVERAGE: host goldens in `tests/dsl_goldens/tests/i18n_packs.rs`;
//! the OS loop is proven by `apphost: locale <tag> applied` (visible boot).
//! RFC: docs/rfcs/RFC-0077-i18n-v2-locale-packs-runtime-switch.md

/// The app's ACTIVE locale source (RFC-0077): pack catalog over the
/// baked default. A macro (not a method) so the immutable field borrows
/// stay disjoint from `&mut self.view` at every dispatch site.
macro_rules! app_locale {
    ($s:expr) => {
        nexus_dsl_runtime::CatalogOverBaked {
            catalog: $s.active_catalog.and_then(|i| $s.catalogs.get(i)).map(|(_, c)| c),
            symbols: &$s.symbols,
            keys: &$s.keys,
        }
    };
}
pub(crate) use app_locale;

/// The NXIR inside a bundle payload WITHOUT parsing the packs — for
/// pre-mount readers (window intent). An NXLC container is NOT a program:
/// reading it raw silently yields default tags (the greeter-in-a-window
/// regression), so every `ProgramReader` consumer must go through here.
pub(super) fn payload_nxir(payload: &'static [u8]) -> &'static [u8] {
    nexus_dsl_runtime::i18n::parse_payload_container(payload).map(|(n, _)| n).unwrap_or(payload)
}

/// Splits a bundle payload into NXIR + `(tag, Catalog)` pairs. The payload
/// may be an `NXLC` container (NXIR + locale packs); parsing is fail-closed —
/// a malformed pack only loses that locale (baked default), never the program.
pub(super) fn split_payload(
    payload: &'static [u8],
) -> (&'static [u8], alloc::vec::Vec<(alloc::string::String, nexus_dsl_runtime::Catalog)>) {
    match nexus_dsl_runtime::i18n::parse_payload_container(payload) {
        Some((nxir, packs)) => (
            nxir,
            packs
                .iter()
                .filter_map(|p| {
                    nexus_dsl_runtime::Catalog::from_indexed_pack(p.pack)
                        .map(|c| (alloc::string::String::from(p.tag), c))
                })
                .collect(),
        ),
        None => (payload, alloc::vec::Vec::new()),
    }
}

/// Reads the program's i18n key table (key index → baked-text symbol id) —
/// the terminal of the locale chain. Absent table = empty (pre-i18n apps).
pub(super) fn i18n_key_table(nxir: &[u8]) -> alloc::vec::Vec<u32> {
    match nexus_dsl_ir::read::ProgramReader::from_canonical_bytes(nxir).and_then(|r| {
        r.root().map(|root| root.get_i18n_keys().map(|l| l.iter().map(|k| k.get_key()).collect()))
    }) {
        Ok(Ok(keys)) => keys,
        _ => alloc::vec::Vec::new(),
    }
}
