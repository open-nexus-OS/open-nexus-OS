// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Host test of init's capability hygiene (TASK-0324 P4f-6, RFC-0093 §4). A
//! `cap_transfer` DUPLICATES the capability into the target (derived rights), so init never needs a
//! clone to hand a capability over — a clone taken for that purpose is an init slot that is never
//! freed (about fifteen of them accumulated before P4f-6, in a table that already runs near its
//! ceiling by wiring time). A clone is legitimate only when it is MOVED in a message
//! (`MsgHeader::new(<clone>, …, CAP_MOVE, …)` or handed on as `Some(<clone>)` to a CAP_MOVE
//! request) or closed. This test scans `src/bootstrap/` and rejects every other `cap_clone`.
//! OWNERS: @runtime
//! STATUS: Production (TASK-0324 P4)

/// Name bound to the clone taken on `lines[idx]` at column `col`, if any.
fn clone_binding(lines: &[&str], idx: usize, col: usize) -> Option<String> {
    let line = lines[idx];
    let ident = |text: &str| -> String {
        text.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect()
    };
    // `.and_then(|clone| …)` on the clone call: the closure parameter holds it (an error mapper such
    // as `.map_err(|_| …)` does not receive the clone).
    if let Some(at) = line[col..].find("and_then(|") {
        let name = ident(&line[col + at + "and_then(|".len()..]);
        if !name.is_empty() && name != "_" {
            return Some(name);
        }
    }
    // `let Ok(clone) = … cap_clone(…)` / `if let Ok(clone) = …`.
    if let Some(ok) = line[..col].rfind("Ok(") {
        let name = ident(&line[ok + 3..]);
        if !name.is_empty() {
            return Some(name);
        }
    }
    // `let clone = … cap_clone(…)` on this line or a few lines above (a `match` arm).
    for back in 0..=5 {
        let Some(at) = idx.checked_sub(back) else { break };
        let text = if back == 0 { &line[..col] } else { lines[at] };
        if let Some(pos) = text.rfind("let ") {
            let rest = text[pos + 4..].trim_start();
            let rest = rest.strip_prefix("mut ").unwrap_or(rest);
            let name = ident(rest);
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

/// 1-based line numbers of `cap_clone` calls in `source` whose clone is neither moved nor closed.
fn clone_leaks(source: &str) -> Vec<usize> {
    let lines: Vec<&str> = source.lines().collect();
    let squeeze = |text: &str| text.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let mut leaks = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        let Some(col) = line.find("cap_clone(") else { continue };
        let rest = squeeze(&lines[idx..].join("\n"));
        let consumed = clone_binding(&lines, idx, col).is_some_and(|var| {
            [format!("MsgHeader::new({var},"), format!("Some({var})"), format!("cap_close({var})")]
                .iter()
                .any(|needle| rest.contains(needle.as_str()))
        });
        if !consumed {
            leaks.push(idx + 1);
        }
    }
    leaks
}

#[test]
fn test_reject_clone_leak() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bootstrap");
    let mut found = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("read src/bootstrap") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_some_and(|ext| ext == "rs") {
            let source = std::fs::read_to_string(&path).expect("read bootstrap source");
            for line in clone_leaks(&source) {
                found.push(format!("{}:{line}", path.display()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "cap_clone results neither moved nor closed (a transfer needs no clone): {found:?}"
    );
}

#[test]
fn test_reject_clone_leak_scanner_catches_the_leak_shapes() {
    // The shapes P4f-6 removed from init must be reported …
    let clone_then_pin = "let c = nexus_abi::cap_clone(req)?;\npin_route_send(pid, a, b, c);";
    let if_let_clone = "if let Ok(clone) = nexus_abi::cap_clone(x) {\n    pin(pid, clone);\n}";
    let closure_clone = "let s = nexus_abi::cap_clone(x).ok().and_then(|clone| pin(pid, clone));";
    for leak in [clone_then_pin, if_let_clone, closure_clone] {
        assert_eq!(clone_leaks(leak), [1], "not reported: {leak}");
    }
    // … and the legitimate CAP_MOVE / close shapes must not be.
    let moved = "let r = nexus_abi::cap_clone(x)?;\nlet hdr = MsgHeader::new(\n    r,\n    0, 0, CAP_MOVE, 4);";
    let moved_as_option =
        "let moved = nexus_abi::cap_clone(vmo).map_err(|_| Fail::Vmo)?;\nrequest(bnd, Some(moved), op);";
    let match_arm = "let cap = match moved_cap {\n    Some(vmo) => vmo,\n    None => nexus_abi::cap_clone(reply).ok()?,\n};\nlet hdr = MsgHeader::new(cap, 0, 0, CAP_MOVE, 4);";
    let closed = "let r = nexus_abi::cap_clone(x)?;\nlet _ = nexus_abi::cap_close(r);";
    for ok in [moved, moved_as_option, match_arm, closed] {
        assert!(clone_leaks(ok).is_empty(), "false positive: {ok}");
    }
}
