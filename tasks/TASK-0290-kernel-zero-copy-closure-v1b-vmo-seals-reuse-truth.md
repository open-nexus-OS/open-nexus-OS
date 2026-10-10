---
title: TASK-0290 Kernel zero-copy closure v1b: VMO sealing rights + write-map denial + reuse truth
status: Draft
owner: @runtime @kernel-team @ui
created: 2026-04-13
depends-on:
  - TASK-0031
follow-up-tasks: []
links:
  - Vision: docs/architecture/vision.md
  - Playbook: CLAUDE.md
  - Zero-copy VMO plumbing baseline: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - UI/kernel perf floor: tasks/TASK-0054B-ui-v1a-kernel-ui-perf-floor-zero-copy-qos-hardening.md
  - IPC fastpath floor: tasks/TASK-0054C-ui-v1a-kernel-ipc-fastpath-control-plane-vmo-bulk.md
  - MM reuse floor: tasks/TASK-0054D-ui-v1a-kernel-mm-perf-floor-vmo-surface-reuse.md
  - Driver optimization prototype: tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

> **Dependency edge corrected (2026-08-14):** the former `depends-on: TASK-0054B/0054C/0054D`
> was backwards — 0054B and 0054D are now Superseded (their goals landed via 0042/0277/0283/
> 0288 and 0310/0309/0302 respectively) and their **residuals fold INTO this task**: the
> reuse/fallback counter floor (ex-0054D goal 5) and the zero-copy-truth accounting
> (ex-0054B goal 1). 0054C (IPC fastpath) is unrelated to VMO sealing and no longer gates
> this task either.

`TASK-0031` correctly treats v1 as plumbing plus honesty, but it explicitly leaves an important gap:
read-only sealing is still a userspace convention rather than a kernel-enforced contract.

For production-grade kernel claims we need:

- kernel-enforced write-map denial for sealed VMOs,
- stable rights semantics at transfer/map time,
- and truthful reuse/copy-fallback evidence for hot paths that claim "zero-copy".

## Goal

Close the kernel side of the zero-copy contract so large-payload data planes are both safer and
more honest:

- sealed VMOs have enforceable rights,
- write mappings are rejected deterministically,
- and reuse/copy-fallback counters make hot-path regressions visible.

### Input from Block 1 (2026-10-09) — the app-host frame arena's end form

app-host's frame arena (ADR-0065) is a STATIC 1 MiB today: two regions (emit, layout) × two
halves = 256 KiB per frame and region, paid in full by every app process because the bytes are
committed at spawn. Measured on the host over the real apps
(`tests/dsl_apps_conformance/tests/frame_arena_budget.rs`, TASK-0251 finding 8): the chat's
first page needs 270 192 B of scene and 418 552 B of layout per frame, so it spills on the
board (`SELFTEST: frame arena spill FAIL`, tolerated in `config/fail-marker-allow-board.txt`
until this lane). The reference capability OS has no fixed frame arena: its heap sits on
demand-paged VMOs, and the UI runtime's young generation starts at 1–2 MB and grows to
8–16 MB per half. The end form here is the same, built by this lane, not bumped before it
(operator 2026-10-09: no interim fix for what this lane replaces): with **M2** (demand paging)
the arena is a generously reserved range committed only where a frame touches it; with **M5**
(purgeable VMOs) a reset generation's pages are purgeable under pressure; with **M4** the
process's limit bounds it.

## Non-Goals

- DMA/IOMMU isolation.
- Full device-fence / dma-buf style ownership graph.
- Replacing higher-level app/service buffer protocols.
- Claiming every bulk path is zero-copy in v1.

## Constraints / invariants (hard requirements)

- **Kernel-enforced rights**: sealing semantics must no longer rely purely on library convention.
- **Truth over branding**: "VMO-capable" and "zero-copy hot path" remain distinct claims.
- **Bounded counters**: reuse/fallback metrics must be stable and cheap enough for production use.
- **No oversized control-plane fallback by default** for payloads that should live on the data plane.

## Red flags / decision points (track explicitly)

- **RED (seal semantics)**:
  - define exactly when a VMO becomes immutable and which transitions are allowed.
- **YELLOW (counter placement)**:
  - keep kernel counters minimal; richer analysis belongs in userland perf tooling.
- **GREEN (scope)**:
  - this is the kernel closeout of the v1 zero-copy contract, not a full media/driver buffer redesign.

## Security considerations

### Threat model
- Receiver gains write access to data advertised as sealed/RO.
- Capability transfer preserves more rights than intended.
- Zero-copy claims hide expensive or unsafe copy fallbacks.

### Security invariants (MUST hold)
- Sealed RO VMOs cannot be write-mapped after seal.
- Map/transfer operations clamp to capability rights.
- Counters do not leak payload contents or secret data.

### DON'T DO (explicit prohibitions)
- DON'T keep "sealed RO" as documentation-only for production-grade claims.
- DON'T treat a copy fallback as success-equivalent in hot-path evidence.
- DON'T expose raw payload data through diagnostics.

## Contract sources (single source of truth)

- Zero-copy plumbing baseline: `TASK-0031`
- UI/kernel bulk-path consumers: `TASK-0054B`, `TASK-0054C`, `TASK-0054D`

## Stop conditions (Definition of Done)

- **Proof (Host)**:
  - kernel/unit tests prove:
    - seal transitions are deterministic,
    - write-map attempts after seal return the documented denial,
    - rights clamping and size checks remain correct,
    - reuse/copy-fallback counters update deterministically on fixed fixtures.
- **Proof (OS/QEMU)**:
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=210s ./scripts/qemu-test.sh`
  - required markers:
    - `KSELFTEST: vmo seal deny ok`
    - `KSELFTEST: vmo rights clamp ok`
    - `SELFTEST: zero-copy closure ok`

## Touched paths (allowlist)

- `source/kernel/neuron/src/mm/`
- `source/kernel/neuron/src/cap/`
- `source/kernel/neuron/src/syscall/`
- `source/libs/nexus-abi/`
- `source/apps/selftest-client/`
- `source/services/windowd/`
- `source/services/*` (only minimal counter consumers if needed)
- `docs/storage/vmo.md`
- `docs/architecture/01-neuron-kernel.md`
- `scripts/qemu-test.sh`

## Plan (small PRs)

1. Define seal/right transition semantics and denial codes.
2. Implement kernel enforcement for write-map denial after seal.
3. Add bounded reuse/copy-fallback counters for representative hot paths.
4. Prove seal denial and zero-copy closure in host/QEMU fixtures.

## Acceptance criteria (behavioral)

- RO-sealed VMO behavior becomes a real kernel contract.
- Hot-path "zero-copy" claims are backed by rights enforcement and measurable reuse/fallback truth.
- UI/media/driver follow-ons can rely on a production-grade kernel data-plane floor.

## Evidence (to paste into PR)

- QEMU: VMO seal-deny and zero-copy closure marker excerpt.
- Tests: exact kernel/unit test summary for rights and counters.
