// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `nx-dsl explain <NXcode>` — the one-line meaning of every diagnostic code.
//! The CODE is the stability contract (`docs/dev/dsl/cli.md`); this text is
//! not, so it may be reworded freely as long as every code the checker can
//! emit still has an entry.

use std::process::ExitCode;

pub(crate) fn cmd_explain(args: &[String]) -> ExitCode {
    let Some(code) = args.first() else {
        eprintln!("nx-dsl explain: pass a diagnostic code (e.g. NX0405)");
        return ExitCode::from(2);
    };
    let text = match code.as_str() {
        "NX0001" => "Unexpected character in the source.",
        "NX0002" => "String literal not closed before end of line/file.",
        "NX0003" => "Source file exceeds the size bound.",
        "NX0004" => "Identifier exceeds the length bound.",
        "NX0005" => "Numeric literal out of range (Int is i64; Fx is Q32.32).",
        "NX0101" => "The parser found a token it cannot use here; the message names what it expected.",
        "NX0103" => "The same property is set twice on one node.",
        "NX0104" => "Content after the last declaration.",
        "NX0105" => "Structural nesting exceeds the bound (64 levels).",
        "NX0106" => "`reduce`/`match` needs at least one arm.",
        "NX0107" => "Route paths start with `/`.",
        "NX0201" => "Name is not defined anywhere visible.",
        "NX0202" => "The same name is defined twice.",
        "NX0203" => "Two imports define the same symbol.",
        "NX0204" => "Not a known widget or a declared component.",
        "NX0205" => "Not a catalog modifier (see docs/dev/dsl/modifiers.md).",
        "NX0206" => "Not a declared event type/case.",
        "NX0207" => "Not a platform service (the surface is generated from dsl_services.capnp).",
        "NX0208" => "The service exists but has no such method.",
        "NX0209" => "Unknown slot: the component declares no `slot` by that name (RFC-0084).",
        "NX0301" => "Types don't match.",
        "NX0302" => "Wrong number of arguments/bindings.",
        "NX0303" => "Unknown field/prop on this type.",
        "NX0304" => "`reduce`/`match` must cover every case.",
        "NX0305" => "Not a case of this enum/event.",
        "NX0306" => "Unknown type name.",
        "NX0307" => "A constant expression is required here.",
        "NX0401" => "Collection items need a stable `.key(expr)` on the template root.",
        "NX0402" => "Interactive nodes need an accessible name (label prop or `.label(…)`).",
        "NX0403" => "A modifier is applied twice on one node.",
        "NX0404" => "`for` needs a statically bounded iterable; use `List(expr) { item in … }` for data.",
        "NX0405" => "Reducers are pure: no IO, no `svc.*`, no dispatch — use an `@effect`.",
        "NX0406" => "Profile branch without a final `else`: add the default branch. (Warning)",
        "NX0407" => "A service result is ignored; bind and handle it. (Warning in v0.1)",
        "NX0408" => "The same route path is declared twice.",
        "NX0409" => "Service calls should pass `timeoutMs:` explicitly. (Warning in v0.1)",
        "NX0410" => "Query outside the v1 shape: eq/>=/<= only, ranges on the orderBy column, literal-or-param values, limit 1..=1000.",
        "NX0411" => "Slot misuse: slot blocks belong on a component that declares them, `Slot x` only inside that component, and a slot body cannot forward its host's slots (RFC-0084).",
        "NX0501" => "Valid syntax, but outside the v0.1 lowering subset (see the task notes).",
        _ => {
            eprintln!("nx-dsl explain: unknown code `{code}`");
            return ExitCode::from(1);
        }
    };
    println!("{code}: {text}");
    ExitCode::SUCCESS
}
