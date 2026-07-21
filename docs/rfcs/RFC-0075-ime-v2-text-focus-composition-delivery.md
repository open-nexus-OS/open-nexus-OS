# RFC-0075: IME v2 — text focus, composition and delivery contract

- Status: In Progress (Phase 0 proven on host 2026-07-21)
- Owners: @ui
- Created: 2026-07-21
- Last Updated: 2026-07-21
- Links:
  - Tasks: `tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md` (host core),
    `tasks/TASK-0147-ime-text-v2-part1b-osk-focus-a11y-os-proofs.md` (OS wiring + OSK),
    `tasks/TASK-0149-ime-v2-part2-cjk-engines-userdict.md` (CJK engines),
    `tasks/TASK-0150-ime-v2-part2b-candidate-popup-osk-cjk-os-proofs.md` (candidate UI),
    `tasks/TASK-0203-ime-v2_1a-host-adaptive-ranking-training-export.md` /
    `tasks/TASK-0204-ime-v2_1b-os-statefs-personal-dict-ui-cli-selftests.md` (personalization)
  - ADRs: `docs/adr/0051-declarative-wire-codec-nexus-wire.md` (frames! codec)
  - Related RFCs: `docs/rfcs/RFC-0058-ui-v3b-clip-scroll-effects-ime-contract.md`
    (stub contract, superseded by this RFC for everything beyond the TASK-0059 baseline),
    `docs/rfcs/RFC-0053-input-v1_0b-os-input-pipeline.md` (input chain)

## Status at a Glance

- **Phase 0 (host core: ime-core + focus model + wire codecs)**: ✅ (TASK-0146 Done 2026-07-21)
- **Phase 1 (OS typing path: imed real, commit delivery)**: ✅ (TASK-0147 Part 1, boot-proven 2026-07-21 — `imed: ready` + `SELFTEST: imed reject foreign ok` in `ci-os-smp1`; interactive typing = `just start` proof until the Part 2 OSK selftest)
- **Phase 2 (OSK overlay app ime-ui)**: ⬜ (TASK-0147 Part 2)
- **Phase 3 (CJK engines + candidate UI)**: ⬜ (TASK-0149/0150)
- **Phase 4 (personalization: ranking + statefs store)**: ⬜ (TASK-0203/0204)

Definition:

- “Complete” means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean “never changes again”.

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The text-focus model (surface focus vs widget focus, announcement protocol).
  - The imed wire protocol (`nexus-wire/src/imed.rs`) and the display-proto text
    ops (`OP_SURFACE_TEXT`, `OP_SURFACE_TEXT_FOCUS`).
  - Composition semantics: dead-key/compose, preedit/commit ordering, engine
    trait boundaries, candidate paging bounds.
  - Security invariants for typed text (identity gates, password fields, logging ban).
  - The IME marker ladder.
- **This RFC does NOT own**:
  - Keymap tables (RFC-0052/TASK-0252, shipped in `userspace/keymaps`).
  - Raw input transport hidrawd→inputd→windowd (RFC-0053).
  - Locale/i18n propagation (`OP_SURFACE_REGION`, RFC-0077) and settings keys (RFC-0078).
  - Wall-clock/timezone (RFC-0076).
  - bidi/shaping (TASK-0148, Deferred).

### Relationship to tasks (single execution truth)

- Tasks define **stop conditions** and **proof commands**; this RFC pins the
  contracts they implement. On contract disputes this RFC wins; on progress
  claims the task ledger wins.

## Context

The OS resolves keystrokes to characters (`userspace/keymaps` in inputd) but
**drops them** — apps only ever receive raw HID key codes via windowd. DSL text
entry is a positional stopgap (`text_input(x, y, text)`), and the imed service
is an empty stub. There is no dead-key support, no composition, no OSK, no CJK,
and typing does not work in QEMU. This RFC pins the production contract that
closes that gap.

## Goals

- One canonical text path: `inputd → imed → windowd → focused surface → app`.
- A two-level focus model with the app as widget-focus authority.
- Deterministic, bounded composition (Latin dead keys now, CJK engines later)
  behind one engine trait.
- Honest end-to-end proofs (typing observed app-side, not service-side).

## Non-Goals

- windowd never hosts IME UI or IME state machines (compositor stays UI-free).
- No a11y announcements in this line (a11y track).
- No cross-device/language-model features; engines are const-table based.

## Constraints / invariants (hard requirements)

- **Determinism**: same key sequence + same engine ⇒ same preedit/candidates/commit.
- **No fake success**: `imed: ready` only after samgr registration + serve loop
  armed; SELFTEST markers only on app-side observation of the committed text.
- **Bounded resources**: preedit ≤ 64 B; compose sequence ≤ 4 keys; candidates
  ≤ 8 × 32 B per frame (paged); fixed `[u8; N]` frames; no per-key allocation
  in OS services.
- **Security floor**: see Security considerations — sender-identity gates and
  the typed-text logging ban hold from the first commit onward.
- **Stubs policy**: none planned; if a phase ships partial behavior it must be
  labeled `stub` in markers, never `ok`.
- **Hot-path protection**: imed sits only on the key/text branch; the pointer
  path (111 Hz budget) is untouched; input chain-stats counters are the
  regression signal.

## Proposed design

### Focus model (normative)

1. **Surface focus**: windowd authority (pointer/touch-down hit-test — existing).
2. **Widget focus**: app authority. The DSL runtime focuses a TextField on tap;
   the app announces transitions upward via `OP_SURFACE_TEXT_FOCUS`
   (focused flag, `field_kind` 0=text/1=password, caret rect x/y/w/h in
   surface coordinates — the caret rect anchors the candidate strip and OSK).
3. windowd relays the aggregate to imed (`OP_SET_FOCUS`), resolving the
   sender's surface by kernel identity — an app can never claim focus for a
   foreign surface. **imed is the key gate**: inputd forwards resolved keys
   unconditionally (human-rate, fire-and-forget) and imed drops them while
   unfocused — no windowd→inputd focus channel exists or is needed.
4. Focus loss commits any active preedit; a pending Latin dead-key accent
   (never visible) is discarded on focus transitions instead — nothing may
   leak across fields.

### Wire contract (normative once Phase 1 proofs are green)

`nexus-wire/src/imed.rs` — MAGIC `'I','E'`, VERSION 1, `frames!` codec,
golden-byte tests + full truncation/mutation reject matrix per op:

| Op | Direction | Payload |
|----|-----------|---------|
| `OP_SET_FOCUS=1` | windowd→imed | `surface_id:u64, focused:u8, field_kind:u8, caret x/y/w/h:u16×4` |
| `OP_KEY=2` | inputd→imed (hw), ime-ui host→imed (osk) | `source:u8 (0=hw,1=osk), kind:u8 (0=text,1=dead,2=action), ch:u32, action:u8, modifiers:u8` |
| `OP_COMMIT=3` | imed→windowd push | `surface_id:u64, text:str8(max 64)` |
| `OP_PREEDIT=4` | imed→windowd push | `surface_id:u64, text:str8(max 64), caret:u8` |
| `OP_CANDIDATES=5` | imed→windowd push | `surface_id:u64, page:u8, count:u8, cand[≤8]:str8(max 32)` |
| `OP_CANDIDATE_SELECT=6` | windowd→imed | `index:u8` |
| `OP_ACTION=7` | imed→windowd push | `surface_id:u64, action:u8` (editing action that passed through composition — windowd translates to `SURFACE_TEXT_ACTION`) |

Status codes: `OK=0, MALFORMED=1, DENIED=2, UNSUPPORTED=3`. Unknown op ⇒ reject.
Additive evolution only; version bump on any breaking change.

`nexus-display-proto/src/client_surface.rs` (windowd↔app push channel):

| Op | Direction | Payload |
|----|-----------|---------|
| `OP_SURFACE_TEXT=21` | windowd→app | `kind:u8 (0=commit,1=preedit,2=action), payload:str8(max 64), aux:u8` |
| `OP_SURFACE_TEXT_FOCUS=22` | app→windowd | `focused:u8, field_kind:u8, caret x/y/w/h:u16×4` |

### Composition semantics (normative)

- Dead key (`^ ´ \`` on DE) enters compose state; next Text key resolves via
  const compose table; unmatched pair falls back to emitting both characters;
  Escape cancels compose; compose state is bounded (≤ 4 pending).
- Preedit lifecycle: engines may hold preedit (CJK); commit on candidate
  selection, Enter, or focus loss. Latin dead-key composition commits directly
  (no visible preedit in Phase 1).
- `ImeEngine` trait (Phase 3): key event in → deterministic
  preedit/candidates/commit snapshot out; engine selection follows the
  `input.keymap` setting; stable candidate ordering (score → table order →
  codepoint) once ranking lands (Phase 4).

### UI ownership (normative)

OSK and candidate strip live in the `ime-ui` DSL overlay app
(`userspace/apps/ime-ui`, `WIN_LEVEL_OVERLAY`). windowd composites and
shows/hides only. This is a hard boundary; the legacy shell keyboard-card
visual is retired in Phase 2.

### Phases / milestones (contract-level)

- **Phase 0**: host core — ime-core compose machine, DSL focused-field model,
  wire codecs + goldens (TASK-0146).
- **Phase 1**: OS typing path — imed real, `ime` placeholder deleted, commit
  delivery proven app-side (TASK-0147 Part 1).
- **Phase 2**: OSK overlay app + policyd injection gate (TASK-0147 Part 2).
- **Phase 3**: CJK engines + candidate strip (TASK-0149/0150).
- **Phase 4**: personalization — deterministic ranking + statefs store,
  toggle/forget (TASK-0203/0204; encryption-at-rest = TASK-0300 seed).

## Security considerations

- **Threat model**: key injection by a non-keyboard service (synthetic input →
  credential theft); typed-text leakage via logs/markers/telemetry; malformed
  frames from any peer; learning as a side channel for secrets.
- **Mitigations (normative invariants)**:
  - imed accepts `OP_KEY` only from inputd's kernel `sender_service_id`;
    `source=osk` keys only from the app-host identity hosting the `ime-ui`
    bundle, policyd-gated (deny-by-default).
  - `OP_SET_FOCUS` / `OP_CANDIDATE_SELECT` accepted only from windowd.
  - Apps trust `OP_SURFACE_TEXT` only on windowd's established push channel.
  - `field_kind=password`: no preedit push, no candidate strip, no learning —
    fail-closed at imed, not at the UI.
  - **Typed text never appears in logs, markers, or debug output.** Selftests
    use fixed fixture strings.
  - All frame fields bounded before parsing (codec-enforced); decoders
    fail-closed; no `unwrap`/`expect` on any frame.
- **Open risks**: OSK injection gate depends on stable bundle identity for
  ime-ui (policyd rule keyed on app-host instance identity) — pinned in
  TASK-0147 Part 2 with `test_reject_*` proof.

## Failure model (normative)

- Unknown/malformed frame ⇒ status `MALFORMED`, state unchanged.
- `OP_KEY` without active focus ⇒ `DENIED`ish no-op (drop, no state).
- Focus loss with active preedit ⇒ commit-then-clear (never silent discard).
- imed unavailable ⇒ typing degrades to nothing (no raw-code fallback into
  text fields — no half-path that bypasses composition); windowd routing of
  raw key codes for shortcuts is unaffected.
- No silent fallback anywhere; every fallback above is explicit and tested.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p ime-core -p keymaps -p nexus-wire -p nexus-display-proto
cd /home/jenning/open-nexus-OS && cargo test -p nexus-dsl-runtime focus
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

### Deterministic markers

- `imed: ready` — serve loop armed on the routed endpoint (Phase 1; upgrades
  the RFC-0058 stub marker semantics, same string)
- `SELFTEST: imed reject foreign ok` — deterministic CI negative proof
  (Phase 1): a foreign-identity `OP_KEY` is DENIED; proves imed serves AND
  the identity gate holds every boot
- `SELFTEST: ime v2 latin us ok` — injected `a` observed as commit app-side
  (QMP-injection lane / OSK-driven in Phase 2 — not in the deterministic CI
  ladder; the interactive proof is typing in `just start`)
- `SELFTEST: ime v2 deadkeys de ok` — `´`+`e` → `é` end-to-end (same lane)
- `SELFTEST: ime v2 osk ok` — OSK tap → commit at focused field (Phase 2)
- `imed: reject foreign key source` — negative injection selftest (Phase 2)
- `SELFTEST: ime v2 cjk jp ok`, `SELFTEST: ime v2 candidates ok` (Phase 3)
- `SELFTEST: ime ranking persist ok` (Phase 4)

## Alternatives considered

- **imed as a library inside inputd** — rejected: couples CJK engine state and
  allocations into the hot input loop; violates the one-authority-per-concern
  registry (TRACK-AUTHORITY-NAMING pins `imed` as the IME authority).
- **windowd-rendered OSK/candidates** — rejected: windowd is a compositor
  service; UI belongs in widgets/apps (standing architecture boundary).
- **Callback-capability IME client API** — rejected for v2: push-over-existing
  channels is simpler and proven (theme push pattern); revisit only with a
  concrete multi-client need.
- **Raw-keycode fallback when imed is down** — rejected: a silent second text
  path would bypass password-field guarantees.

## Open questions

- OSK bundle identity for the policyd injection rule: app-host instance id vs
  bundle id — decide in TASK-0147 Part 2 (owner @ui) before the gate lands.

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: ime-core + focused-field model + wire codecs — proof: `cargo test -p ime-core -p keymaps -p nexus-wire -p nexus-display-proto` + DSL runtime focus tests (green 2026-07-21; evidence in TASK-0146)
- [x] **Phase 1**: imed real + typing lands in apps + `ime` deleted — proof: boot ladder (`init: up imed`, `imed: ready`) + `SELFTEST: imed reject foreign ok` (green in `ci-os-smp1` 2026-07-21); positive typing chain interactive until Phase 2's OSK selftest
- [ ] **Phase 2**: ime-ui OSK + policyd gate — proof: `SELFTEST: ime v2 osk ok`, `imed: reject foreign key source`
- [ ] **Phase 3**: CJK engines + candidate strip — proof: `SELFTEST: ime v2 cjk jp ok`, `SELFTEST: ime v2 candidates ok`
- [ ] **Phase 4**: ranking + statefs store — proof: `SELFTEST: ime ranking persist ok`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` + `tools/nx/chains/markers.txt` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`: foreign key source, non-windowd focus, password-field guards).
