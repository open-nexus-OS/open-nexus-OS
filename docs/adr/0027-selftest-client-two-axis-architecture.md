# ADR-0027: selftest-client Two-Axis Architecture

Status: Accepted
Date: 2026-04-17
Owners: @runtime

## Context

`source/apps/selftest-client` is the canonical end-to-end OS proof harness:
its UART output (the "marker ladder") is what `scripts/qemu-test.sh` greps to
gate every release. Through TASK-0021, TASK-0022, and TASK-0023 the client
grew organically: by the start of TASK-0023B the `os_lite::run()` body was
~1100 lines of inline orchestration in a single `os_lite/mod.rs` (1256 LoC
total), interleaving:

- per-service IPC bring-up,
- routing/cap-move probes,
- OTA stage / switch / rollback state-machine pumping,
- policy allow/deny + audit-record proofs,
- exec lifecycle + minidump proofs,
- logd hardening + metrics/tracing facade proofs,
- kernel-IPC plumbing + security probes,
- MMIO + cap query proofs,
- VFS, ICMP, DSoftBus, remote (resolve / pkgfs / statefs) proofs,
- and the cooperative idle endgame.

This shape made every new task widen `run()`, every regression hard to bisect,
and every cross-cut change (e.g. RFC-0019 nonce-correlated reply pumps) prone
to behavioral drift, because the marker emission order WAS the contract and
the only way to change one slice was to read all of it.

We needed a structure that:

1. Lets the marker ladder remain **byte-identical** across refactors (any
   marker order/content change is a contract change, not a refactor).
2. Lets new tasks (TASK-0024 DSoftBus QUIC recovery, TRACK-PODCASTS-APP, the
   TASK-0023B Phase-4 manifest, `SMP=2` IPI proof, etc.) land **without
   touching `run()`** — the only allowed `run()` edit is appending one more
   `phases::<name>::run(&mut ctx)?` line.
3. Maps cleanly onto the runtime/harness profile dimensions that Phase 4 of
   TASK-0023B introduces (`SELFTEST_PROFILE=full|bringup|quick|ota|net|none`
   on the runtime side; `full|smp|dhcp|os2vm|quic-required` on the harness
   side), without forcing recompiles per profile.
4. Keeps proofs deterministic on RISC-V `no_std`/`os-lite` (no `std`,
   no `parking_lot`, no `getrandom`, no fake success).

## Decision

Adopt a **two-axis structure** under `source/apps/selftest-client/src/os_lite/`:

### Axis 1 — Capability nouns (what)

Each capability the selftest exercises owns its own subtree. Each subtree is
self-contained: it owns its IPC clients, frame builders, reject mappers, and
local helpers, but **emits no markers itself** — it returns
`Result<…, E>` and exposes `pub(crate)` functions for orchestrators to call.

```
os_lite/
├── services/        # per-service IPC clients (samgrd, bundlemgrd, keystored,
│                    #   policyd, execd, logd, metricsd, statefs, bootctl)
├── ipc/             # cross-service IPC primitives
│                    #   (clients/, routing/, reply/, reply_inbox/)
├── probes/          # focused proof primitives
│                    #   (rng, device_key, elf, core_service,
│                    #    ipc_kernel/{plumbing, security, soak})
├── dsoftbus/        # DSoftBus client + QUIC OS transport + remote/{resolve,
│                    #   pkgfs, statefs}
├── net/             # ICMP ping, local-addr resolution, optional smoltcp probe
├── mmio/            # MMIO bring-up + cap query
├── vfs/             # cross-process VFS proof
├── timed/           # timed-coalesce probe
└── updated/         # OTA helper family
                     #   (types, reply_pump, stage, switch, status, health)
```

### Axis 2 — Orchestration verbs (when)

A new sibling `phases/` directory turns the temporal sequence into explicit
modules. Each phase owns a contiguous slice of the original `run()` body and
emits the slice's markers in the slice's original order.

```
os_lite/
├── context.rs       # PhaseCtx (cross-phase state, minimal by rule)
├── mod.rs           # 31 LoC: mod-decls + 14-line `pub fn run()` dispatch
└── phases/
    ├── bringup.rs       # keystored + qos + timed + rng + device-key +
    │                    #   statefs CRUD + dsoftbus readiness + samgrd v1
    ├── routing.rs       # policyd / bundlemgrd / updated routing-slot probes
    ├── ota.rs           # TASK-0007 stage / switch / rollback / bootctl
    ├── policy.rs        # allow/deny + MMIO-policy deny + ABI-filter
    ├── exec.rs          # spawn / exit / minidump / forged-metadata reject
    ├── logd.rs          # TASK-0014 hardening + metrics/tracing facade
    ├── ipc_kernel.rs    # orchestration: probes::ipc_kernel::*
    ├── mmio.rs          # TASK-0010 MMIO + cap query
    ├── vfs.rs           # cross-process VFS probe
    ├── net.rs           # ICMP ping + DSoftBus OS transport
    ├── remote.rs        # TASK-0005 resolve / query / statefs / pkgfs
    └── end.rs           # `SELFTEST: end` + cooperative idle
```

### Cross-phase state — `PhaseCtx`

A minimal `PhaseCtx` (`os_lite/context.rs`) holds **only** state that:

- is read by ≥ 2 phases, OR
- directly determines the marker ladder.

Today the locked field set is:

- `reply_send_slot: u32`, `reply_recv_slot: u32` (deterministic shared
  reply-inbox slot pair, RFC-0019),
- `updated_pending: VecDeque<Vec<u8>>` (out-of-order replies pumped across
  routing → ota),
- `local_ip: Option<[u8; 4]>` (resolved in `net`, consumed by `remote`),
- `os2vm: bool` (Node A / Node B 2-VM harness flag).

Service handles are **deliberately** NOT cached on `PhaseCtx`. Each phase
re-resolves the handles it needs via the existing silent
`route_with_retry`. This keeps the phase isolation invariant (see below) real
rather than aspirational, and matches the pre-refactor cost model (`run()`
already re-resolved handles per slice).

### Invariants (mechanically enforced from Phase 3 onward)

1. **Marker parity** — the QEMU `SELFTEST:` ladder must be byte-identical to
   the immediately prior commit unless the commit is explicitly tagged as a
   marker-changing contract update (RFC-0014 / RFC-0038 ladder change).
2. **Phase isolation** — `phases/*` MUST NOT import other `phases::*`.
   Allowed downstream imports: `services::*`, `ipc::*`, `probes::*`,
   `dsoftbus::*`, `net::*`, `mmio::*`, `vfs::*`, `timed::*`, `updated::*`,
   `markers::*`, `crate::os_lite::context::PhaseCtx`. Enforced by
   `scripts/check-selftest-arch.sh` (TASK-0023B Cut P3-03).
3. **Aggregator-only `mod.rs`** — when a folder contains > 1 file, its
   `mod.rs` contains only `mod` declarations and `pub(crate) use`
   re-exports; no `fn` bodies. Single-file folders flatten to `name.rs`
   (Cut P3-01 sweep).
4. **Marker authority** — only `crate::markers` and (Phase 4+) the
   `markers_generated.rs` file produced from `proof-manifest.toml` may
   contain marker string literals. All other modules emit through
   `crate::markers::*` helpers.
5. **No fake success** — `*: ready` and `SELFTEST: * ok` only after the
   asserted behavior actually happens; stubs emit `stub`/`placeholder`,
   never `ok`.
6. **Visibility ceiling** — `pub(crate)` everywhere inside `os_lite`
   (binary crate boundary).
7. **No new `unwrap`/`expect`** in selftest paths; build scripts and tests
   may use them under the standard `clippy::expect_used` allow.
8. **OS build hygiene** — no `parking_lot`, no `getrandom`, no `std` in
   `os-lite` graph (enforced by `just dep-gate`).

### Explicitly rejected alternatives

- `trait Phase` / generic `Probe` trait hierarchy — boilerplate without
  composition gain; the linear deterministic ladder maps onto free
  functions cleanly.
- Hand-written marker-string Rust constants — superseded by Phase 4
  generation from `proof-manifest.toml`, which removes the two-truth
  surface entirely.
- Renaming `os_lite/` to `os_suite/` etc. — 36-file churn for cosmetic
  gain.
- Cfg-time runtime-profile selection — forces a recompile per profile;
  superseded by `SELFTEST_PROFILE` env / kernel cmdline (Phase 4 P4-08).
- Caching service handles on `PhaseCtx` — would conflate Phase 2
  (behavior-preserving extraction) with a separate refactor and break
  the phase isolation invariant in subtle ways.
- Collapsing host-side proofs into the same orchestration tree — host
  tests (`cargo test --workspace`, `just test-host`,
  `just test-dsoftbus-quic`) stay outside the QEMU manifest by design;
  cargo-tested host logic and QEMU-attested OS behavior have different
  failure modes and recovery costs.

## Consequences

### Positive

- `os_lite/mod.rs` shrank 1256 → **31 LoC**; `pub fn run()` body shrank
  ~1100 → **14 lines** of phase dispatch.
- New tasks land as a new file under one of the capability nouns plus, at
  most, one extra `phases::<name>::run(&mut ctx)?` line in `os_lite/mod.rs`
  (often zero — the new file slots into an existing phase).
- The marker ladder is now reviewable at the `phases/` granularity: a
  diff to `phases/ota.rs` says "OTA proof slice changed", not "selftest
  changed somewhere".
- `PhaseCtx` minimality keeps the per-phase mental model small and makes
  the phase isolation rule mechanically checkable.
- The two-axis split maps directly onto the Phase-4 marker manifest
  (`[phase.X]` / `[marker."…"]` / `[profile.Y]` sections), so the same
  structure carries through Phases 4-6 (signed evidence + replay).

### Negative

- More files to navigate (~62 `.rs` files under `os_lite/` after
  Phase-2 sub-splits). Mitigated by the README onboarding guide
  (`source/apps/selftest-client/README.md`).
- Mechanical re-resolution of service handles per phase trades a few
  cheap routing calls for the isolation invariant. Acceptable: routing
  is silent and deterministic, and the original code already paid this
  cost in most slices.
- Single source of truth for `ReplyInboxV1` (`ipc/reply_inbox.rs`) means
  any future shared-inbox semantics change has exactly one edit point;
  this is the intent, but it does mean cross-cut RFC-0019 changes
  affect three callers (`cap_move_reply_probe`, `sender_pid_probe`,
  `ipc_soak_probe`) atomically.

### Risks

- Drift between RFC-0014's documented phase list (12 phases after Cut
  P2-00) and `scripts/qemu-test.sh`'s `PHASES` array (still 8 today).
  Acknowledged in RFC-0014 §3 and resolved at Phase 4 / Cut P4-05 when
  the harness consumes `proof-manifest.toml`.
- New contributors may try to import across `phases::*` for convenience.
  Mechanically caught by `scripts/check-selftest-arch.sh` (Cut P3-03);
  until then enforced by code review.

## Cross-references

- **Contract / cut tables**: [docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md)
- **Execution SSOT**: [tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md](../../tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md)
- **QEMU phase contract**: [docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md)
- **Reply-inbox correlation**: [docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md](../rfcs/RFC-0019-ipc-request-reply-correlation-v1.md)
- **General service architecture**: [docs/adr/0017-service-architecture.md](0017-service-architecture.md) — orthogonal: ADR-0017 governs *how a daemon is structured*, ADR-0027 governs *how the proof harness orchestrates daemons*.
- **QEMU smoke gating**: [docs/adr/0025-qemu-smoke-proof-gating.md](0025-qemu-smoke-proof-gating.md) — orthogonal: ADR-0025 governs which proofs are required vs optional via env vars; ADR-0027 governs the internal layout that produces those proofs.
- **Onboarding**: [source/apps/selftest-client/README.md](../../source/apps/selftest-client/README.md)

## Current state (2026-04-17)

- TASK-0023B Phase 1 closed (capability extractions).
- TASK-0023B Phase 2 closed (two-axis structure landed in 18 cuts P2-00 → P2-17).
- TASK-0023B Phase 3 in planning (4 cuts: flatten / `host_lite/` extraction
  / `arch-gate` mechanical enforcement / standards review).
- TASK-0023B Phase 4-6 contract locked in RFC-0038 (manifest / signed
  evidence / replay).
