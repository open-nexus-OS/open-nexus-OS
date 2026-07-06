---
title: TASK-0078 DSL v0.2b: typed svc.* adapters from the real IDL + transcript testing + CLI generators + master-detail demo (host)
status: Draft
owner: @ui @runtime
created: 2025-12-23
updated: 2026-07-06
depends-on:
  - tasks/TASK-0077-dsl-v0_2a-state-nav-i18n-core.md
follow-up-tasks:
  - tasks/TASK-0078B-dsl-v0_2b-queryspec-v1-foundation-service-gated-paging-hash.md
links:
  - Track: tasks/TRACK-DSL-V1-DEVX.md
  - Services contract: docs/dev/dsl/services.md
  - Real IDL SSOT the stubs generate from: tools/nexus-idl/schemas/*.capnp
    + userspace/nexus-idl-runtime (generated modules; IPC frame = [opcode u8][capnp])
  - QuerySpec sibling: tasks/TASK-0078B (the demo's list data comes through it)
  - Data formats rubric: docs/adr/0021-structured-data-formats-json-vs-capnp.md
  - Testing contract: scripts/qemu-test.sh
---

## Context (updated 2026-07-06)

Effects become useful: `svc.*` calls execute against **typed adapters generated from
the repo's real IDL** (`tools/nexus-idl/schemas/*.capnp` — the same schemas every
service speaks). This closes the gap the old spec left open ("typed handles from
schema/IDL" was aspirational): the DSL service surface **is** the platform IDL, not a
parallel definition.

Host-first: adapters run against **record/replay transcripts** (deterministic recorded
request→response exchanges), so full app logic is provable without the OS. The real
IPC `EffectHost` lands in the app-host (TASK-0080D); this task keeps everything
host-side. OS markers formerly listed here move to Phase 6 (launch e2e).

## Goal

1. **svc adapter layer** (`userspace/dsl/runtime/src/svc/`, generated + thin manual
   glue — no new crate): typed clients for an initial service set (`appState`
   (statefsd-backed contract), `search`, `users` demo schema), uniform
   `Result<T, ErrCode>` mapping, mandatory `timeoutMs`, byte/row budgets.
   `EffectHost` impls: `TranscriptHost` (host: record/replay) — real IPC host later.
2. **Frontend knowledge of services**: `svc.<service>.<method>` typechecks against the
   generated signatures (unknown service/method/arity = stable diagnostic).
3. **CLI upgrades** (`nx-dsl`):
   - `run` gains `--route/--locale/--profile` (headless run against transcripts);
   - `i18n extract <appdir> -o i18n/en.json` (authoring view) + `i18n compile`
     (binary catalog);
   - generators, minimal scaffold posture: `init`, `add page|component|store|service|
     test`, `session inspect|clear|export --json` (host debugging);
   - lint growth: i18n coverage, route uniqueness (already Error), transcript
     staleness warning.
4. **Example app `examples/dsl/masterdetail`**: routes `/` + `/detail/:id`, list loads
   via a `svc` call (swaps to QuerySpec paging when 0078B lands), `en`+`de` catalogs,
   windowed list; becomes the Phase-6 launch-demo payload and the shared list/detail
   proof target.

## Non-Goals

- Real IPC execution (TASK-0080D app-host). QuerySpec itself (TASK-0078B). Full
  service coverage. OS markers/selftests (Phase 6). Kernel changes.

## Constraints / invariants (hard requirements)

- Adapters generated from the IDL SSOT — no hand-maintained parallel signatures.
- Deterministic effect scheduling with bounded concurrency; every call has an explicit
  timeout; stable error-code enums (no stringly failures).
- Transcripts are checked-in fixtures; replay is byte-deterministic; recording only
  via an explicit dev flag (no fake success: a replay miss is a test failure, never a
  silent default).
- No `unwrap/expect`; no godfiles; no company/product names.

## Stop conditions (Definition of Done)

### Proof (Host) — required

`tests/dsl_v0_2_host/`:

- transcript call: `LoadRequested` → adapter replay → `Loaded` state update
  (deterministic golden);
- timeout + error paths: transcripted `Err` and timeout produce the canonical
  error-state transitions (stable codes);
- unknown service/method fixture = stable frontend diagnostic;
- master-detail: nav to `/detail/7` snapshot golden; locale switch golden;
  `--profile desktop` vs `--profile tv` stable distinct snapshots;
- `nx dsl run` exits 0 with expected output; generators produce buildable scaffolds
  (init → build green);
- conformance corpus extended (effect call/err/timeout cases).

### Docs — required (reference grade)

- `docs/dev/dsl/services.md` to full reference (adapter generation, transcripts,
  budgets, error-code discipline); `cli.md` complete for the new verbs/generators.

## Touched paths (allowlist)

- `userspace/dsl/runtime/` (svc/ module + EffectHost transcript impl)
- `userspace/dsl/core/` (svc signature typecheck), `userspace/dsl/cli/` (verbs +
  generators)
- `examples/dsl/masterdetail/` (new), `tests/dsl_v0_2_host/` (new)
- `tools/nexus-idl/schemas/` (demo `users.capnp` if needed)
- `docs/dev/dsl/{services,cli}.md`

## Plan (small PRs)

1. adapter generation from IDL + TranscriptHost + typecheck integration
2. error/timeout discipline + fixtures
3. CLI verbs + generators + scaffold tests
4. master-detail demo + i18n packs + goldens + docs

---

## STATUS / PROGRESS LEDGER (updated 2026-07-06)

### ✅ DONE (first increment — pulled forward with the 0077 finish)

- **CLI project mode**: `nx-dsl build|… <appdir>` walks `ui/**.nx` (sorted paths) through
  `merge_project` + `canonical_source_set` — platform overrides included. Single-file mode
  unchanged.
- **`examples/dsl/masterdetail/`** — the canonical multi-file app (and the intended
  Phase-6 launch-demo payload): `ui/composables/library.store.nx` (store/events/reduce/
  effect), `ui/pages/{ListPage,DetailPage,Routes}.nx`, `ui/platform/phone/pages/
  DetailPage.nx` override. Exercises routes + `navigate` handlers + keyed List + Card +
  two-way TextField binding + `@t` keys + effect-loaded data. Scene test proves:
  project builds to ONE `.nxir`; desktop list-tap → detail → back; the phone fixture
  renders the override layout from the same bytes.

### ⬜ OPEN (this task's core)

- Typed `svc.*` adapters generated from the real IDL schemas (frontend signature checks
  NX0302/unknown-service; currently svc calls are opaque and host-scripted).
- TranscriptHost (record/replay transcript files; the conformance `Script` host is the
  in-memory precursor).
- `i18n extract|compile` verbs; generators (`init`, `add …`); `run --route/--locale/--profile`.
- en/de catalogs for masterdetail (runtime `Catalog` machinery is ready).
