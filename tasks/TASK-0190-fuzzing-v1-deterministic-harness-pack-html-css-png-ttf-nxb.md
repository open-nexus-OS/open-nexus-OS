---
title: TASK-0190 Fuzzing v1 (host-only): deterministic harness pack (HTML/CSS/PNG/TTF/NXB) + corpora + CI smoke gate + docs
status: Draft
owner: @security
created: 2025-12-27
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - WebView sanitizer/parser risk surface: tasks/TASK-0176-webview-net-v1a-host-sanitizer-webview-sceneir-goldens.md
  - WebView v1.1 sanitizer CSP strict: tasks/TASK-0186-webview-v1_1a-host-webview-core-history-find-sessionstorage-csp.md
  - Packaging manifest parser risk surface: tasks/TASK-0129-packages-v1a-nxb-format-signing-pkgr-tool.md
  - PNG/TTF assets pipeline (renderer): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
---

## Context

We have multiple parser-heavy surfaces where memory safety and denial-of-service risks concentrate:

- HTML/CSS (sanitizer),
- PNG decode,
- TTF parsing,
- NXB manifest parsing/validation.

We want deterministic, offline fuzz *smoke* runs to catch panics and invariants violations early.
This is host-only by design.

Related planning:

- Some security prompts require “fuzz gates” without external engines. For those surfaces we use deterministic corpus-driven tests
  (plain `cargo test`) as a complement to `cargo-fuzz` smoke; tracked in `TASK-0231`.

## Goal

Deliver:

1. `cargo-fuzz` targets (host-only) with deterministic smoke config:
   - `html_sanitizer_fuzz`: never produces script nodes; no panic; bounded allocations
   - `css_tokenizer_fuzz`: allowlist tokenizer invariants; no panic
   - `png_decoder_fuzz`: rejects oversize images deterministically; no OOM
   - `ttf_parser_fuzz`: bounded glyph/outline parsing; no panic
   - `nxb_manifest_fuzz`: manifest reader/validator rejects malformed inputs deterministically
2. Corpora:
   - seed corpora under a repo-tracked directory with small deterministic inputs
   - optional dictionaries for better coverage
3. CI smoke gate:
   - run each fuzzer with fixed seed and fixed run count (e.g., `-runs=5000 -seed=1337`)
   - document that this is a *smoke* gate, not exhaustive coverage
4. Docs:
   - how to run fuzzers deterministically
   - how to add corpus seeds safely and keep them small

## Non-Goals

- Running fuzzers in QEMU/OS.
- Treating fuzz smoke as proof of correctness (it is a regression detector).

## Constraints / invariants (hard requirements)

- Fuzz targets must be bounded:
  - cap input sizes,
  - cap decoded image dimensions,
  - cap recursion depth / node counts.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Red flags / decision points (track explicitly)

- **YELLOW (determinism of fuzzing)**:
  - libFuzzer can still vary by platform/compiler. The CI “smoke gate” must be treated as best-effort regression detection, not a formal proof signal.

## Stop conditions (Definition of Done)

- `cargo fuzz run <target> -- -runs=5000 -seed=1337` succeeds for all targets on CI baseline builder.

## Touched paths (allowlist)

- `fuzz/` (new)
- fuzz corpora directory (new)
- `docs/security/fuzzing.md`

## Plan (small PRs)

1. add fuzz workspace + 2 targets (html/css) + corpora + docs
2. add png/ttf/nxb targets + corpora
3. add CI smoke script and document limits

## Acceptance criteria (behavioral)

- Deterministic fuzz smoke runs complete with zero crashes and bounded resource usage.
