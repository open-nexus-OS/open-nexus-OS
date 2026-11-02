---
schema: 1
id: TASK-0146
title: "IME v2 Part 1a (host-first): ime-core dead/compose engine + DSL focused-field model + imed/display-proto wire codecs"
status: done
status_note: 2026-07-21
owner: ["@ui"]
created: 2025-12-26
updated: 2026-07-21
subsystem: ui
done: 2026-07-21
dates_source: status-text
depends_on: [TASK-0059, TASK-0252, TASK-0296]
follow_ups: [TASK-0147]
implements: [RFC-0075]
links:
  - label: Vision
    target: docs/architecture/vision.md
  - label: Playbook
    target: CLAUDE.md
  - label: "Contract seed"
    target: "docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md (seeded by this task)"
  - label: "Authority & naming registry"
    target: tasks/TRACK-AUTHORITY-NAMING.md
  - label: "Keymap tables (Done, reused)"
    target: tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel.md
  - label: "IME/text-input plumbing baseline (Done)"
    target: tasks/TASK-0059-ui-v3b-clip-scroll-effects-ime-textinput.md
  - label: "Wire codec pattern"
    target: docs/adr/0051-declarative-wire-codec-nexus-wire.md
  - label: "Testing contract"
    target: scripts/qemu-test.sh
---

> updated (migrated 2026-11-02): 2026-07-21 (rewritten against repo reality; architecture per RFC-0075)

## Context

Repo reality (verified 2026-07-21, supersedes the 2025-12 draft):

- `userspace/keymaps/` (TASK-0252, Done) already ships complete US-QWERTY and
  DE-QWERTZ tables (`Keymap::resolve(usage, mods) → KeyOutput::{Text, Action}`,
  AltGr, äöüß/€). The previously planned `userspace/libs/ime-keymaps` crate is
  **stale** — this task reuses `userspace/keymaps` and only adds dead-key marking.
- `source/services/imed/` is the TASK-0059 stub (in-memory `TextFocus` +
  `CaretSelection`, no wire protocol, empty `os_entry`). It becomes real in
  TASK-0147; this task builds everything host-provable beneath it.
- inputd already resolves `KeyOutput::Text(char)` per keystroke but **drops it**
  (`source/services/inputd/src/service.rs::apply_keyboard`); only raw HID codes
  reach windowd. DSL text entry is positional
  (`userspace/dsl/runtime/src/view.rs::text_input(x, y, text)`) — no focus model.
- Wire protocols are declared via the `frames!` codec in `source/libs/nexus-wire`
  (ADR-0051); the windowd→app push channel is
  `source/libs/nexus-display-proto/src/client_surface.rs` (ops ≤ 20 taken; 21–23 free).

## Goal

Host-provable IME foundation, in four pieces:

1. **`userspace/ime-core`** (new, no_std, zero deps): dead-key/compose state
   machine (DE dead keys `^ ´ \``, bounded compose table), preedit buffer +
   commit output, deterministic `ImeOutcome { handled, preedit, commit }` API.
2. **`userspace/keymaps` extension**: `KeyOutput::Dead(char)` marking for dead
   keys on the DE table (additive; existing resolve semantics unchanged).
3. **DSL focused-field model** (`userspace/dsl/runtime`): Tap on
   TextField/TextArea focuses it (caret, insert/backspace/enter ops,
   focus-change surfaced to the host); replaces the positional `text_input`
   path. `GlassTextField` gains focused + caret rendering states.
4. **Wire codecs** (`frames!`, golden bytes + reject matrix):
   - `source/libs/nexus-wire/src/imed.rs` — MAGIC `'I','E'` v1: `OP_SET_FOCUS=1`
     (windowd→imed), `OP_KEY=2` (inputd→imed, source-tagged hw/osk),
     `OP_COMMIT=3` / `OP_PREEDIT=4` / `OP_CANDIDATES=5` (imed→windowd push),
     `OP_CANDIDATE_SELECT=6`.
   - `source/libs/nexus-display-proto/src/client_surface.rs` —
     `OP_SURFACE_TEXT=21` (windowd→app: commit/preedit/action, str8 ≤ 64 B),
     `OP_SURFACE_TEXT_FOCUS=22` (app→windowd: focused, field_kind
     text/password, caret rect).

## Non-Goals

- No OS wiring, no samgr registration, no QEMU markers (TASK-0147).
- No CJK engines (TASK-0149), no candidate UI (TASK-0150), no OSK (TASK-0147).
- No bidi/shaping (TASK-0148, Deferred).
- No changes to the hot pointer path in inputd.

## Constraints / invariants (hard requirements)

- Determinism: no timing-based assertions; compose tables are const data.
- Bounded state: preedit ≤ 64 B, compose sequence ≤ 4, candidates ≤ 8×32 B.
- `ime-core` is no_std, `#![forbid(unsafe_code)]`, alloc-free, zero deps.
- Wire additions are additive (both protocols keep VERSION, unknown ops reject);
  every decoder fail-closed (`None`), reject matrix per protocol.
- No `unwrap`/`expect` on untrusted input; no blanket `allow(dead_code)`.

## Security considerations

### Threat model
- Malformed imed/display-proto frames from less-trusted services.
- Typed text is sensitive by definition (passwords, private messages).

### Security invariants (MUST hold)
- Typed text never appears in logs, markers, or debug output — not in this
  crate, not downstream (contract pinned in RFC-0075).
- `field_kind=password` is carried in `OP_SURFACE_TEXT_FOCUS` from day one so
  downstream consumers (imed learning, preview) can fail closed on it.
- All frame fields bounded before use; str8 length limits enforced in the codec.

### DON'T DO
- DON'T log preedit/commit contents anywhere (test fixtures use fixed strings).
- DON'T accept unbounded compose sequences or preedit growth.

## Contract sources (single source of truth)

- **Wire contract**: golden-byte tests + reject matrices in
  `source/libs/nexus-wire/src/imed.rs` and nexus-display-proto.
- **Composition semantics**: RFC-0075 (seeded here, Draft until TASK-0147 proof).

## Stop conditions (Definition of Done)

- **Proof (host)**:
  - `cargo test -p ime-core` — compose goldens (`´`+`e`→`é`, `^`+`a`→`â`,
    `´`+`x`→ fallback `´x`, Escape cancels), preedit/commit ordering, bounds.
  - `cargo test -p keymaps` — DE dead-key marking; existing contract tests untouched.
  - `cargo test -p nexus-wire -p nexus-display-proto` — golden bytes + reject matrices.
  - DSL runtime tests — tap-to-focus, insert/backspace/enter, focus loss on
    surface change, positional path removed.
- **Proof (gates)**: `just check` + `just test-host` green; structure gate clean.

## Touched paths (allowlist)

- `userspace/ime-core/` (new: src/ + tests/)
- `userspace/keymaps/` (additive: Dead marking)
- `userspace/dsl/runtime/` (focused-field model), `userspace/ui/widgets/text_field/`
- `source/libs/nexus-wire/src/imed.rs` (new) — **approval zone**
- `source/libs/nexus-display-proto/src/client_surface.rs` (ops 21–22) — **approval zone**
- `docs/rfcs/RFC-0075-*.md` (new seed) — **approval zone**
- `docs/dev/ui/input/{ime.md,text-input.md}` (stubs → real docs), `Cargo.toml` (workspace member)

## Plan (small PRs)

1. RFC-0075 seed (contract: wire ops, focus model, marker ladder, security).
2. `ime-core` crate: compose/dead-key machine + preedit/commit + tests.
3. `keymaps`: `KeyOutput::Dead` + DE dead-key rows + tests.
4. Wire codecs: imed.rs + display-proto ops 21/22, goldens + reject matrices.
5. DSL runtime focused-field model + GlassTextField states + tests.

## Acceptance criteria (behavioral)

- US/DE sequences (incl. dead keys) produce deterministic preedit/commit on host.
- A DSL view with two TextFields: tapping either moves focus, typing edits only
  the focused one, focus loss commits preedit.
- Every new wire op has a golden vector and a full truncation/mutation reject matrix.
- No OS/QEMU markers claimed in this part.

## RFC seeds

- RFC-0075 "IME v2 — text focus, composition and delivery contract" (this task
  seeds it; TASK-0147 proves it in QEMU and flips it toward Complete).

## Evidence (2026-07-21)

- `cargo test -p ime-core`: 12 passed (compose goldens `´`+`e`→`é`, `^`+`a`→`â`,
  fallback `´x`, same-dead-twice, rearm, Escape/Backspace cancel, Enter
  flush-and-pass, reset-on-focus-loss, deterministic fixture ×2, preedit
  bounds, keymap mapping).
- `cargo test -p input_v1_0_host --test keymaps_contract`: 5 passed incl. new
  `keymaps_de_marks_dead_keys` (DE EQUAL/GRAVE = Dead, US unchanged).
- `cargo test -p nexus-wire imed`: 7 passed (golden bytes, candidate-list
  bounds, response, full truncation/mutation reject matrix).
- `cargo test -p nexus-display-proto`: 18 passed (surface_text roundtrips +
  reject paths in the new `surface_text.rs` module — client_surface.rs back at
  its 965-line baseline, structure-gate clean).
- `cargo test -p dsl_goldens --test scenes`: 14 passed incl. new
  `tap_to_focus_insert_backspace_and_secure` (focus, insert `hé`, backspace,
  secure snapshot, scene never contains the secret, empty-tap clears, no-op
  insert without focus). Focus model lives in `runtime/src/focus.rs` (view.rs
  back under the 600-line ratchet).
- `just check` green (fmt, clippy, deny, arch-check, structure-gate).
- `just test-host` green (full workspace). Side fix: latent test-compile break
  in `nexus-gfx/command/buffer_wire_tests.rs` (`super::buffer` from nested
  module, landed 2026-07-20) repaired.
- No OS/QEMU markers claimed (per Non-Goals); OS wiring = TASK-0147.
