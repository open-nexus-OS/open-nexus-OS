// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Generates the frontend's `svc.*` signature table from the IDL
//! SSOT (`tools/nexus-idl/schemas/dsl_services.capnp`) — the checker can
//! never disagree with the platform surface because it is derived from it.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Exercised by every core test that checks a `svc.*` call

// Build scripts panic-to-fail by design: an unwrap/expect here aborts the
// build with a clear message, which is the correct failure mode (there is no
// caller to propagate to). expect_used/unwrap_used are host-lint denials meant
// for library/service code, not build-time codegen.
#![allow(clippy::expect_used, clippy::unwrap_used)]

use std::fmt::Write as _;
use std::path::Path;

fn main() {
    // Runtime env (NOT `env!`): the compile-time macro bakes the manifest dir
    // of whichever environment compiled this script — host and container
    // builds share `target/`, so a baked absolute path poisons the other
    // side's cache (/workspace vs the host checkout).
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let schema =
        Path::new(&manifest_dir).join("../../../tools/nexus-idl/schemas/dsl_services.capnp");
    println!("cargo:rerun-if-changed={}", schema.display());
    let text = std::fs::read_to_string(&schema)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", schema.display()));
    let entries = parse_surface(&text);
    assert!(!entries.is_empty(), "dsl_services.capnp: no surface entries parsed");

    let mut out = String::from(
        "// GENERATED from tools/nexus-idl/schemas/dsl_services.capnp — do not edit.\n\
         /// One platform service method visible to the DSL.\n\
         pub struct SvcSig {\n\
         \x20   pub service: &'static str,\n\
         \x20   pub method: &'static str,\n\
         \x20   /// DSL argument type names, positional.\n\
         \x20   pub args: &'static [&'static str],\n\
         \x20   /// DSL result type name (the Ok payload).\n\
         \x20   pub result: &'static str,\n\
         }\n\
         /// The full surface, sorted by (service, method).\n\
         pub const SVC_SURFACE: &[SvcSig] = &[\n",
    );
    for (service, method, args, result) in &entries {
        let args_rs: Vec<String> = args.iter().map(|a| format!("\"{a}\"")).collect();
        let _ = writeln!(
            out,
            "    SvcSig {{ service: \"{service}\", method: \"{method}\", args: &[{}], result: \"{result}\" }},",
            args_rs.join(", ")
        );
    }
    out.push_str("];\n");

    let dest = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("svc_surface.rs");
    std::fs::write(&dest, out).unwrap_or_else(|e| panic!("cannot write {}: {e}", dest.display()));
}

/// Parses the `const dslSurface` entries. The schema file pins the exact
/// entry style (see its STYLE CONTRACT header), so this stays a simple,
/// fail-loud text parse — a malformed entry aborts the build.
fn parse_surface(text: &str) -> Vec<(String, String, Vec<String>, String)> {
    let mut entries = Vec::new();
    let Some(start) = text.find("const dslSurface") else {
        panic!("dsl_services.capnp: `const dslSurface` block not found");
    };
    for raw in text[start..].split('(').skip(1) {
        let Some(end) = raw.find(')') else { continue };
        let entry = &raw[..end];
        if !entry.contains("service") {
            continue;
        }
        let field = |name: &str| -> String {
            let key = format!("{name} = \"");
            let Some(pos) = entry.find(&key) else {
                panic!("dsl_services.capnp: entry misses `{name}`: {entry}");
            };
            let rest = &entry[pos + key.len()..];
            rest[..rest.find('"').expect("closing quote")].to_string()
        };
        let args = {
            let Some(pos) = entry.find("args = [") else {
                panic!("dsl_services.capnp: entry misses `args`: {entry}");
            };
            let rest = &entry[pos + "args = [".len()..];
            let inner = &rest[..rest.find(']').expect("closing bracket")];
            inner
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_matches('"').to_string())
                .collect::<Vec<_>>()
        };
        entries.push((field("service"), field("method"), args, field("result")));
    }
    entries
}
