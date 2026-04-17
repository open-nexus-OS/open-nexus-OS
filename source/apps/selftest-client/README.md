# selftest-client

> The canonical end-to-end OS proof harness for Open Nexus OS.
> Its UART output (the "marker ladder") is what `scripts/qemu-test.sh` greps to gate every release.

If a behavior is not covered by a marker emitted from this app, it is **not** part of the boot-time proof contract.

---

## TL;DR

| | |
|---|---|
| **Purpose** | Drive every userspace service from inside the OS, prove behavior with stable UART markers, fail loud and early on regressions. |
| **Architecture** | Two-axis: capability **nouns** (`services/`, `ipc/`, `probes/`, `net/`, `dsoftbus/`, `mmio/`, `vfs/`, `timed/`, `updated/`) + orchestration **verbs** (`phases/<name>::run(&mut PhaseCtx)`). |
| **Entry point** | `os_lite::run()` — 12-phase dispatch (see [`src/os_lite/mod.rs`](src/os_lite/mod.rs)). |
| **Architectural contract** | [`docs/adr/0027-selftest-client-two-axis-architecture.md`](../../../docs/adr/0027-selftest-client-two-axis-architecture.md) |
| **Refactor RFC** | [`docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md`](../../../docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) |
| **Run it** | `just test-os` (single VM) · `tools/os2vm.sh` (2-VM cross-VM proofs) |

---

## Two flavors of the same crate

`selftest-client` builds in two mutually exclusive shapes selected via Cargo features. **Pick the right one for your task.**

### `--features os-lite` — the OS harness (the one that gates releases)

* `no_std`, RISC-V, runs as a userspace task spawned by `nexus-init` inside the OS image.
* All of `src/os_lite/**` is active.
* Talks to live services (samgrd, bundlemgrd, policyd, execd, logd, metricsd, statefs, keystored, dsoftbusd, netstackd, bootctl, updated, vfsd, packagefsd, timed, rngd) over kernel IPC.
* Output: stable `SELFTEST: ...` marker ladder on UART.
* This is what `just test-os` exercises.

### `--features std` (default) — the host harness

* `std`, runs on your dev machine.
* Used for fast unit-shaped checks against host-compatible service crates (no kernel, no IPC).
* Has no marker ladder; failures surface as normal Rust test/exit codes.

> **Rule of thumb:** if you are touching something that needs a kernel, an IPC slot, or a hardware-shaped device path, you want `os-lite`. If you can prove it with pure logic on the host, prefer `std` (or, even better, a unit test in the owning crate).

---

## Architecture in 60 seconds

``` text
src/os_lite/
├── mod.rs              ← entry: 12 mod decls + 14-line pub fn run() dispatcher
├── context.rs          ← PhaseCtx (the only cross-phase state)
├── phases/             ← VERBS — orchestration; each owns a slice of the ladder
│   ├── bringup.rs      (1)  keystored / qos / rng / device-key / statefs / samgrd
│   ├── routing.rs      (2)  routing-slot announcements + service routing probes
│   ├── ota.rs          (3)  stage / switch / rollback A/B
│   ├── policy.rs       (4)  policyd allow/deny / MMIO-policy / requester-spoof
│   ├── exec.rs         (5)  execd ELF spawn / minidump / rejects
│   ├── logd.rs         (6)  logd anchors / metrics / journaling
│   ├── ipc_kernel.rs   (7)  kernel-IPC plumbing / security / soak
│   ├── mmio.rs         (8)  MMIO mapping policy (USER|RW, never X)
│   ├── vfs.rs          (9)  cross-process VFS probe
│   ├── net.rs          (10) netstackd local-addr + ICMP + DSoftBus QUIC subset
│   ├── remote.rs       (11) cross-VM remote proxy proofs (2-VM only)
│   └── end.rs          (12) SELFTEST: end + cooperative idle loop
├── services/           ← NOUNS — IPC clients per daemon (one mod per service)
├── ipc/                ← NOUNS — shared IPC primitives
│   ├── clients.rs        cached resolved clients
│   ├── routing.rs        route_with_retry + bounded waits
│   ├── reply.rs          one-shot reply slot helpers
│   └── reply_inbox.rs    ReplyInboxV1 — RFC-0019 nonce-correlated shared inbox
├── probes/             ← NOUNS — focused proof primitives (rng, elf, device_key, ipc_kernel/*)
├── dsoftbus/           ← NOUNS — DSoftBus QUIC + cross-VM remote (resolve / statefs / pkgfs)
├── net/                ← NOUNS — netstackd helpers (local_addr, icmp_ping, smoltcp opt-in)
├── mmio/               ← NOUNS — MmioBus + W^X reject path
├── vfs/                ← NOUNS — verify_vfs() over kernel IPC
├── timed/              ← NOUNS — timed coalesce probe
└── updated/            ← NOUNS — OTA helpers (stage / switch / status / health / reply pump)
```

### Two invariants, hold them or break the harness

1. **Phase isolation.** `phases::*` modules **must not import other `phases::*` modules.** Cross-phase data flows through `PhaseCtx` (`context.rs`). Service handles are re-resolved per phase. This is what keeps the dispatcher honest and lets you reorder, skip, or parallelize phases later without spooky cross-coupling.
2. **`mod.rs` is aggregator-only.** Files named `mod.rs` under `os_lite/` contain only `mod` declarations and `pub(crate) use` re-exports — **no `fn` bodies**. New behavior goes into a sibling submodule with a clear noun/verb name.

The full rationale, rejected alternatives, and consequences are in [ADR-0027](../../../docs/adr/0027-selftest-client-two-axis-architecture.md).

---

## How to run

### Single-VM smoke (the default release gate)

```bash
just test-os
```

Behind the scenes this:
1. Builds the OS image (kernel + nexus-init + selftest-client + all services).
2. Boots QEMU with `virt` + virtio-mmio (modern, not legacy).
3. Streams UART to a log and `grep -E '^SELFTEST: '` against the expected ladder.
4. Fails fast on any missing marker, out-of-order marker, or `FAIL`/`PANIC` prefix.

Useful knobs:

| Env / flag | Effect |
|---|---|
| `RUN_UNTIL_MARKER=1` | Stop QEMU as soon as the last expected marker shows up (faster local dev). |
| `REQUIRE_DSOFTBUS=1` | Promote the DSoftBus QUIC-subset markers from "best effort" to "required". |
| `--features smoltcp-probe` | Build-time: enable the bring-up smoltcp probe (`net/smoltcp_probe.rs`). Off by default to avoid drift. |

### Cross-VM proofs (TASK-0005, opt-in)

```bash
tools/os2vm.sh
```

Boots two QEMU instances on a bridge. Only Node A emits the `dsoftbusd_remote_*` markers (`phases::remote`); single-VM smoke is **expected** to skip this phase.

### Host harness

```bash
cargo test -p selftest-client          # default features = std
just diag-host                         # workspace-wide host check
```

---

## The marker ladder is the contract

* Every gating marker has the prefix `SELFTEST:` (followed by a space) and lives in `crate::markers`.
* Markers are **stable strings** — no random IDs, no timestamps, no counts that vary run-to-run.
* The order is locked by `pub fn run()` in `src/os_lite/mod.rs` plus the body of each phase.
* `*: ready` and `SELFTEST: * ok` are emitted **only after a real assertion or verified end condition**. Stubs say `stub`/`placeholder`, never `ok`.
* Negative paths get their own markers (`SELFTEST: ... reject ok`) — a missing reject marker is a security regression, not a flake.

If you change a marker string or order, you are changing the public contract of the harness. Update `scripts/qemu-test.sh` in the same commit and call it out explicitly.

---

## Adding a new proof — decision tree

Before writing code, answer two questions:

**1. Is it a new capability primitive (a noun)?**
For example: a new IPC client for a new service, a new low-level probe.
→ Add a file under the matching noun directory: `services/<svc>/mod.rs`, `ipc/<thing>.rs`, `probes/<thing>.rs`, `dsoftbus/...`, `net/...`, etc.
→ Keep it marker-free. The probe returns a `Result`; the *caller* (a phase) decides what marker to emit on success/failure.

**2. Is it a new orchestrated step (a verb)?**
For example: a new check that has to happen after `policy` but before `exec`.
→ Either extend an existing `phases/<name>.rs` (preferred when it logically belongs to that slice) **or** add a new `phases/<new>.rs` and wire it into `pub fn run()` in the right slot.
→ The phase function signature is fixed: `pub(crate) fn run(ctx: &mut PhaseCtx) -> Result<(), ()>`.
→ If you need cross-phase state (e.g. a value computed in `net` and read by `remote`), add a field to `PhaseCtx` — and only if it's read by ≥2 phases or directly determines the marker ladder. Otherwise keep it local.

**Then:**

* Add the marker emission in the phase, using `crate::markers::{emit_line, emit_byte, ...}`.
* Add reject-path markers for any negative case you care about.
* Update `scripts/qemu-test.sh` if the marker is gating.
* Run `just diag-os && just test-os` and confirm the new marker shows up in order.

---

## Determinism rules (do not break these)

These are not style preferences; violating them flakes the release gate.

* **Bound everything.** Every IPC retry, every poll, every wait uses an explicit deadline (`nexus_ipc::budget::deadline_after`). No "drain inbox until empty", no "yield until it works".
* **Correlate, don't drain.** For shared reply inboxes use `ipc::reply_inbox::ReplyInboxV1` (RFC-0019 nonce correlation). Never assume reply order.
* **No nondeterministic markers.** No timestamps, no random IDs, no counts that vary between runs in any string that the gate greps.
* **No fake success.** A `ready` or `ok` marker must be preceded by a real assertion. If a path is degraded, say `stub` / `placeholder` / `degraded` explicitly.
* **No kernel-side debug prints.** `selftest-client` instruments userspace. If the kernel needs proof, expose it via a syscall/IPC and probe it from here.

See `.cursor/rules/12-debug-discipline.mdc` for the full version.

---

## File header convention

Every Rust file in this crate carries a CONTEXT header per `docs/standards/DOCUMENTATION_STANDARDS.md`:

```rust
// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: <one-paragraph description of what this module does>.
//! OWNERS: @runtime
//! STATUS: Functional | Diagnostic / opt-in | Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: <where the proof of this module lives>
//!
//! ADR: docs/adr/0027-selftest-client-two-axis-architecture.md
```

If you add a new file, copy this block. If you split a file, the new pieces inherit the same OWNERS / ADR.

---

## Common pitfalls

* **Adding a `fn` to a `mod.rs` under `os_lite/`.** Don't. Make a sibling submodule. The aggregator-only rule is enforced by reviewer convention; CI doesn't catch it for you.
* **Importing `phases::other` from `phases::self`.** Forbidden — breaks phase isolation. Use `PhaseCtx`.
* **Adding a new field to `PhaseCtx` "just in case".** `PhaseCtx` is intentionally minimal. If only one phase reads it, it's a local variable, not context.
* **Emitting a marker from inside a `probes/*` or `services/*` helper.** Probes return `Result`; phases own the markers. The exception is `vfs::verify_vfs`, which emits its own granular sub-markers and is a documented edge case.
* **Forgetting `extern crate alloc;`** in a new submodule that uses `alloc::*`. The crate is `no_std` for `os-lite`; submodules that touch `Vec`, `VecDeque`, `String`, `Box` need the explicit `extern crate alloc;` at the top of the file.
* **Letting `rustfmt` rewrite unrelated files.** Run `just fmt-check` and only commit formatting churn that belongs to your cut. If `rustfmt` drifts an unrelated file, `git checkout --` it.
* **Editing a marker string without updating `scripts/qemu-test.sh`.** The gate will go red on the next CI run, not in your local check. Grep for the exact string before changing it.

---

## Where to look next

* **Architectural contract** — [`docs/adr/0027-selftest-client-two-axis-architecture.md`](../../../docs/adr/0027-selftest-client-two-axis-architecture.md)
* **Refactor RFC (history + roadmap of phases 3–6)** — [`docs/rfcs/RFC-0038-...`](../../../docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md)
* **QEMU phases / testing contract** — [`docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`](../../../docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md)
* **IPC reply correlation** — [`docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`](../../../docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md)
* **QEMU smoke proof gating (ADR)** — [`docs/adr/0025-qemu-smoke-proof-gating.md`](../../../docs/adr/0025-qemu-smoke-proof-gating.md)
* **Documentation header standard** — [`docs/standards/DOCUMENTATION_STANDARDS.md`](../../../docs/standards/DOCUMENTATION_STANDARDS.md)
* **Debug discipline** — [`.cursor/rules/12-debug-discipline.mdc`](../../../.cursor/rules/12-debug-discipline.mdc)

---

## Status

This crate's structure is the result of TASK-0023B Phase 2 (closed). Phases 3–6 (consolidation of duplicated client patterns, marker manifest as SSOT, signed evidence bundle, replay capability) are tracked in RFC-0038 and the active task file. Behavioral parity — byte-identical marker ladder vs. pre-refactor baseline — is the non-negotiable invariant for every cut.

If you find yourself wanting to "just add it to `os_lite/mod.rs`", stop and re-read the noun/verb decision tree above. The whole point of the current shape is that you don't have to.
