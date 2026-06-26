# ADR-0040: Unified logging policy, timing signposts, and the pure-observer proof model

- Status: Accepted (track: UART observability overhaul + reactive boot + pure-observer selftest).
- Created: 2026-06-26
- Builds on: ADR-0001 (runtime roles), ADR-0025 (QEMU smoke proof gating), ADR-0027 (selftest-client
  two-axis architecture), ADR-0033 (soft-real-time spine: waitset + fence), RFC-0003 (unified
  logging), RFC-0013 (boot gates / readiness), RFC-0014 (testing contracts + QEMU phases).
- Code: `source/libs/nexus-log`, `source/kernel/neuron/src/diag/log.rs`,
  `source/services/logd`, `source/services/metricsd`, `source/apps/selftest-client`,
  `source/libs/nexus-proof-manifest`, `source/libs/nexus-evidence`.

## Context

A full boot emits **~1226 UART lines** with no overview. Measured by prefix, the bulk is raw
debug tracing that bypasses every level/topic gate:

| Prefix | Lines | What it is | How it is emitted |
|---|---|---|---|
| `init:` | 182 | service start/up ladder | raw print |
| `CAP:` | 127 | capability-allocator internals | raw `write!` to the UART writer |
| `AS:` | 71 | address-space activation trace | raw `write_str` to the UART writer |
| `SELFTEST:` | 79 | proof ladder, all "ok" | `emit_line` |
| `dbg:` | 25 | windowd init step-trace | raw print |
| `fps:` | 19 | inputd 30-field counter dumps | raw print |

Three structural facts drive this ADR:

1. **The level infrastructure already exists and is bypassed.** `nexus-log` has five levels
   (Error/Warn/Info/Debug/Trace), a runtime `MAX_LEVEL` (`set_max_level`) and a per-domain
   `TOPIC_MASK` (`set_topic_mask`). The kernel `diag::log` has the same five levels (Debug/Trace
   compile-gated on `debug_assertions`). Yet the loudest lines — `CAP:`/`AS:` (kernel,
   `cap/mod.rs`, `mm/address_space.rs`, `mm/satp.rs`), `dbg:` (windowd), `fps:` (inputd) — are
   raw `write!`/`write_str`/`println!` that never consult a level or topic. That is exactly why
   "build in dev mode" cannot quiet them: there is no gate to turn.

2. **The console is not serialized across emitters.** Kernel `emit()` writes a whole line under
   `KernelUart::lock()`, but userspace records stream out byte-at-a-time via `debug_putc` and
   interleave with the kernel and with each other, producing corrupted lines
   (`UB!init-lite`, `UBkeystored:`, `UBUBvirtio-blk:`).

3. **The proof harness conflates three roles in one task.** `selftest-client` drives every
   service by hand-mirroring its wire protocol, asserts the replies, and emits all 79
   `SELFTEST:` markers. A protocol change forces edits in both the owning service and this
   central poker — the precise double-structure the project works to remove. The architecture
   docs already describe it as an "out-of-band observer that never writes," which the code
   contradicts.

Monotonic time is cheap and already available to every task via the `nsec()` syscall, but it is
used only for timeout deadlines, never for instrumentation. A kernel-only `boot_timing` feature
emits `T:init=`/`T:boot=` cycle counts and nothing reaches userspace.

The goal is the model that reliability-focused systems converge on, described generically here to
keep vendor and product names out of the tree (see the project legal rule): one leveled,
per-subsystem, optionally-timestamped logging facade that every emitter routes through;
Info/Debug off by default with a runtime opt-in that needs no rebuild; interval "signpost" timing
to locate bottlenecks; a structured logging daemon and a telemetry daemon as the steady-state
sinks; and a thin verifier that observes the structured stream rather than driving the system.

## Decision

### 1. One facade, honest levels, quiet by default

Every emitter routes through the facade — `nexus-log` in userspace, `diag::log` in the kernel.
Raw `write!`/`println!`/`debug_putc` tracing is reclassified onto it at an honest level and
topic. No emitter writes the console directly except the facade's own sink.

Default runtime floor is **Warn + a curated Info set** (service lifecycle, `*: ready`, the
boot-timing table). Debug and Trace are **off by default**. The reclassification:

| Source | New level | Topic |
|---|---|---|
| `CAP:` allocator internals | Trace | `cap` |
| `AS:` address-space trace | Trace | `vm` |
| `dbg:` windowd init steps | Debug | `windowd` |
| `fps:` inputd counters | telemetry (see §3) — Debug line only as fallback | `inputd` |
| `init:` / `*: ready` lifecycle | Info | `boot` |

The kernel `diag::log` gains the same runtime `MAX_LEVEL` + `TOPIC_MASK` gate that `nexus-log`
already has, so a debug build no longer means "every trace on at once" — topics are selected, not
all-or-nothing.

### 2. A runtime knob, no rebuild

Verbosity is selected at boot by a single string, e.g.
`NEXUS_LOG="windowd=debug,gpud=info,cap=off"`, parsed once and applied through the existing
`set_max_level` / `set_topic_mask`. This extends the current `INIT_LITE_LOG_TOPICS` build-time
plumbing (`init-lite/build.rs` → `bootstrap/helpers.rs` → `set_topic_mask`;
`nexus-service-entry`) into a runtime control. Marker strings are unchanged, so the proof gate is
unaffected by this step.

### 3. `logd` is the logging daemon; `metricsd` is the telemetry daemon

- **`logd`** is the steady-state structured sink. The `sink-logd` path already exists in
  `nexus-log`; once `logd` is up, records route through it (central per-subsystem filtering and a
  single serialized record stream — the stream the verifier in §4 consumes).
- **`metricsd`** is the steady-state telemetry sink. The `fps:` dumps become named
  counters/gauges instead of a giant UART line; the timing signposts (§5) become spans and
  histograms; the boot-timing table is a `metricsd` dump.

Emission is **layered**: early/critical boot (kernel, init, services before `logd` is up) writes
the direct serialized console; afterward, records flow through `logd`/`metricsd`. Both sinks
degrade gracefully — if absent in a given profile (e.g. `metricsd` is optional in
`scripts/qemu-test.sh`), emitters fall back to the direct console and never block boot.

### 4. Serialized console

The direct-console path writes **one record atomically**: a record is buffered to a line and
written under the kernel UART lock (or a single per-record syscall), so kernel and userspace
records can no longer interleave. This removes the `UB…` corruption regardless of which sink is
active.

### 5. Signpost timing

A small `span!` / signpost helper wraps an interval (begin/end, category) over `nsec()` and
records it into `metricsd` (spans + histograms); it emits to UART only when the `timing` topic is
on. It wraps kernel init (generalizing `boot_timing`), each service `spawn → ready`, and each
proof phase. A compact **end-of-boot timing table** is emitted at Info (per-service `up → ready`
deltas and total boot), which is the data used to find services still self-pacing on a timeout
instead of waking on an event.

### 6. Three-tier proof model; `selftest-client` becomes a pure observer

The proof harness is split by responsibility:

- **Tier 1 — component self-tests.** Each service exercises its own happy, reject, and edge paths
  using its own protocol code (no duplicated wire definitions) and emits structured result
  records through the facade. The proof lives with the code it proves.
- **Tier 2 — a thin integration exerciser.** Only the genuinely cross-service flows (OTA A/B
  across updated + bundlemgr + bootctl; the two-VM remote flows) live in a small dedicated task —
  not a mirror of every service's protocol.
- **Tier 3 — `selftest-client` is a pure observer.** It subscribes to the `logd` structured
  stream (UART as fallback), matches it against the proof-manifest SSOT, and emits only the
  verdict `SELFTEST NN/NN ok` (failures expanded). It sends no IPC and drives nothing.

### 7. The manifest stays the proof SSOT; the verdict transport changes

`nexus-proof-manifest verify-uart` and `nexus-evidence` are reworked to consume the structured
summary the observer produces, keeping **deny-by-default** semantics (an expected-but-missing
result fails; an unexpected result fails). The `proof-manifest/` tree remains the single source of
truth for which results must and must not appear under each profile; only the transport of the
verdict — per-line greps → one structured summary — changes.

## Consequences

- A default `just start` boot drops from ~1226 lines to a few hundred, with a single
  `SELFTEST NN/NN ok` verdict and a boot-timing table; full detail is one `NEXUS_LOG=` away with
  no rebuild.
- Console corruption is eliminated; log lines are atomic.
- A protocol change touches only its owning service's Tier-1 self-test, not a central poker —
  the double-structure is removed and the architecture docs and code finally agree.
- The change is staged so the low-risk steps (verbosity policy, console serialization, timing)
  land without touching marker strings or the release gate; only the Tier-1/observer rework
  (which is isolated and run across every profile) changes the proof transport.
- Boot-timing data exposes timeout-bound waits, enabling the reactive-boot conversions
  (init MMIO-grant yield loops and fixed selftest waits → waitset / reply-correlated event wakes)
  tracked on the same overall track.

## Alternatives considered

- **Just delete the noisy markers.** Rejected: they are valuable when debugging. The problem is
  that they are always-on and ungated, not that they exist. Gating behind levels/topics keeps
  them one env var away.
- **Keep `selftest-client` driving and only summarize its output.** Rejected: it leaves the
  protocol-mirror double-structure in place, which is the core maintainability defect.
- **Move the data-plane log records to the control-plane schema runtime.** Out of scope; the
  per-record hot path stays a compact line, consistent with the wire-codec decision in ADR-0038.
