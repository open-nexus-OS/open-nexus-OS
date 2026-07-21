<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# IME

Contract: `docs/rfcs/RFC-0075-ime-v2-text-focus-composition-delivery.md`.
Execution: TASK-0146 (host core) → TASK-0147 (OS wiring + OSK) →
TASK-0149/0150 (CJK + candidates) → TASK-0203/0204 (personalization).

## Architecture (RFC-0075)

```
hidrawd → inputd ──(keymap resolve, exists)──► imed ──► windowd ──► focused surface → app-host
                     KeyOutput::{Text,Dead,      composition        OP_SURFACE_TEXT     insert into
                     Action} while text-focused  (ime-core)         push                focused field
```

- **imed** is the IME authority (TRACK-AUTHORITY-NAMING); `source/services/ime`
  is deprecated and gets deleted in TASK-0147.
- **ime-core** (`userspace/ime-core`): no_std, alloc-free composition —
  dead-key/compose state machine (DE `´` `` ` `` `^`), bounded preedit,
  deterministic `ImeOutcome`. Engine trait for CJK lands in TASK-0149.
- **Keymaps** stay in `userspace/keymaps` (TASK-0252); dead keys are marked
  `KeyOutput::Dead(char)` and only the composer interprets them.
- **OSK + candidate strip** live in the `ime-ui` DSL overlay app — never in
  windowd (compositor stays UI-free).

## Wire

- `nexus-wire/src/imed.rs` (MAGIC `'I','E'`): `OP_SET_FOCUS` (windowd→imed),
  `OP_KEY` (inputd/OSK→imed), `OP_COMMIT`/`OP_PREEDIT`/`OP_CANDIDATES`
  (imed→windowd push), `OP_CANDIDATE_SELECT`.
- `nexus-display-proto/src/surface_text.rs`: `OP_SURFACE_TEXT` (windowd→app),
  `OP_SURFACE_TEXT_FOCUS` (app→windowd, caret rect = OSK/popup anchor).

## Security invariants

- imed accepts `OP_KEY` only from inputd (`sender_service_id`); OSK keys only
  from the vetted ime-ui host, policyd-gated (deny-by-default).
- Password fields (`field_kind=password`): no preedit push, no candidates,
  no personalization learning — enforced in imed, fail-closed.
- Typed text NEVER appears in logs or markers; selftests use fixed fixtures.

## Markers

`imed: ready` · `SELFTEST: ime v2 latin us ok` · `SELFTEST: ime v2 deadkeys de
ok` · `SELFTEST: ime v2 osk ok` · `imed: reject foreign key source` ·
`SELFTEST: ime v2 cjk jp ok` · `SELFTEST: ime v2 candidates ok` ·
`SELFTEST: ime ranking persist ok` (see the RFC's marker ladder).
