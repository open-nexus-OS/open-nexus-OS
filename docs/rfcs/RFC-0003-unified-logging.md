# RFC-0003: Unified Logging Infrastructure

- Status: In Progress
- Authors: Runtime Team
- Last Updated: 2025-11-29

## Status at a Glance

- **Phase 0 (Deterministic bring-up logging + guards)**: In progress
- **Phase 1 (Kernel sink parity / delegation)**: Planned
- **Phase 2 (Routing, buffers, advanced controls)**: Planned

Definition:

- This RFC is considered “Complete” only when both userspace and kernel use the same facade/sink
  semantics for severity/target, and the panic-safe fallback path is consistent across domains.

## Role in the RFC stack (how RFC-0003 fits with RFC-0004/0005)

RFC‑0003 is **supporting infrastructure**, not a blocker for RFC‑0004/0005:

- **RFC‑0004 depends on RFC‑0003 (minimal)** for provenance-safe, deterministic logging so guard
  violations can be surfaced without dereferencing untrusted pointers.
- **RFC‑0005 depends on RFC‑0003 (minimal)** for honest QEMU markers and actionable diagnostics
  during IPC/policy/routing bring-up.

Practical rule:

- We implement only what we need for **deterministic markers + guard visibility**, and defer the
  “full logging control plane” (buffers/routing) unless it unblocks security or correctness.

“Done enough for now” criteria:

- Core services and init-lite can emit deterministic markers without duplicated helpers.
- Logging guards remain strict (reject non-provenance-safe pointers deterministically).

## Summary

Neuron currently mixes several ad-hoc logging styles (raw UART loops in the
kernel, `debug_putc` wrappers in userspace, temporary panic prints). This RFC
proposes a unified `nexus-log` facade that provides consistent severity/target
semantics across domains, predictable fallbacks during bring-up, and room for
future routing (buffers, tracing).

## Motivation

- **Consistency** – today each component formats its own strings, which makes
  enable/disable switches and tooling brittle.
- **Determinism** – panic paths often fall back to bespoke UART loops; we need a
  shared fallback story that avoids touching allocators or `fmt` when the world
  is on fire.
- **Observability control** – logs grow noisy during bring-up; we need
  compile-time defaults and runtime knobs to focus on the domain under
  investigation.
- **Long-term tooling** – structured logging (key/value, ring buffers) depends
  on a single choke-point that can evolve without auditing every caller.

## Goals

1. Provide a single `no_std` crate (`nexus-log`) with:
   - `Level` abstraction (Error/Warn/Info/Debug/Trace)
   - Domain/target tagging (`[LEVEL target] payload`)
   - Minimal runtime configuration (global max level, later target masks)
2. Separate sinks for kernel and userspace while keeping the API identical.
3. Guarantee a panic-safe path (raw UART) that does not allocate or touch
   `core::fmt` if the caller does not request it.
4. Allow callers to compose lines without relying on trait objects; still expose
   an escape hatch for formatted arguments when safe.
5. Document phased adoption so we can incrementally port existing sites without
   derailing current debugging efforts.

### Non-goals (for this RFC iteration)

- Structured/JSON logging.
- Asynchronous draining or per-core ring buffers (tracked as future work).
- Automatically rewriting existing macros (e.g. `log_info!`) in one sweep.

## Implementation Status

### Completion snapshot (2025-12-12)

- Overall: ~60% complete. Unified facade + topic gating landed; hybrid guard track partially rolled out; kernel sink adoption and full StrRef/VMO provenance still pending.

| Component | Status | Notes |
| --------- | ------ | ----- |
| Userspace sink + facade | **Complete** | `nexus-init` (os-lite backend) routes through `nexus-log`; `init-lite` is a thin wrapper that only forwards into the shared path and calls into the kernel `exec` syscall. |
| Pointer guards (Phase 0a) | **In progress (partially implemented)** | Guard code merged and `nexus-log` rejects non-canonical/guard-crossing strings; loader and VMO provenance still pending under RFC‑0004. |
| Topic filters (Phase 2 preview) | **Partial** | `Topic` bitmask + `INIT_LITE_LOG_TOPICS` env landed; the mask is set by `nexus-init` and init-lite merely forwards, so svc-meta/probe traces are gated centrally. |
| Probe topic gating | **Complete** | Verbose loader/allocator probes now attach to the `probe` topic. The topic is disabled by default so UART output stays deterministic; opt in with `INIT_LITE_LOG_TOPICS=probe,…` when debugging. |
| Kernel exec-path logging | **Partial** | Kernel `exec`/stack mapping now emits gated `exec` topic lines (STACK-MAP / STACK-CHECK) and will adopt the same topic mask as `init-lite`/`nexus-init`. Guarded sink + StrRef plumbing is required next. |
| StrRef / bounded sink buffers | **Planned** | Replace raw `&str` emission with validated handles and optional bounce buffers, inspired by Fuchsia / seL4. |
| StrRef handles (Phase 0a hybrid) | **In progress** | Introduced `StrRef` handle + `LineBuilder::text_ref`, adopted by init-lite’s service-name guard; still need broader rollout and kernel plumbing. |
| Kernel delegation (Phase 1) | **Planned** | Kernel logging still uses legacy macros. |

### Hybrid Guard Track (Phase 0a Interim)

To unblock the current init-lite debugging sprint we are rolling out a hybrid
safety track inspired by seL4/Fuchsia: every userspace slice handed to the
`sink-userspace` backend is now validated in constant time (canonical address,
bounded length, overflow detection) and tagged with the caller’s `ra`, `level`,
and `target`. When a slice fails validation we drop the write, emit a
`guard-*` reason code, and record the precise metadata to simplify blame.
Additionally:

- `trace_large_write` telemetry now prints the emitting `[LEVEL target]` and
  call-site `ra` whenever an unusually large buffer is flushed.
- High-risk call sites (service-name logging, guard probes) copy their payloads
  into small bounce buffers guarded by canaries before invoking `LineBuilder`.
- A `StrRef` handle plus `LineBuilder::text_ref` landed to begin the migration
  away from raw `&str` arguments; init-lite already uses it for service names.

These measures are documented here so we can treat them as part of RFC‑0003’s
Phase 0a completion criteria rather than temporary debugging hacks.

### Probe topic & UART hygiene

Enterprise OS builds keep the boot UART extremely quiet (seL4 defaults to
panic-only prints; Fuchsia mirrors logs into serial only when
`kernel.serial-debuglog` is toggled). To mirror that behaviour, all of our
byte-by-byte loader/allocator probes are now treated as a named topic:

- The `probe` topic is disabled by default so CI and production runs only see
  severity-tagged markers.
- Export `INIT_LITE_LOG_TOPICS=probe,svc-meta` before rebuilding to opt into
  the raw loader diagnostics (`!dbg-word`, `guard-str`, allocator traces, etc.).
- Both the `nexus-init` loader and the `nexus-service-entry` bump allocator
  honour the topic, so a single switch quiets the UART stream without deleting
  the instrumentation.
- Structured `nexus_log::*` records still fire even when the probe topic is
  disabled, so post-mortem tooling can inspect the buffered metadata without
  re-running the build.

This hybrid approach keeps the default logs production-friendly while retaining
the deep-dive probes behind an explicit opt-in, matching the workflows described
above and RFC‑0003’s observability goals.

### Recent Progress (2025-11-29)

- `StrRef` handle + `LineBuilder::text_ref` API introduced; init-lite converts
  its service metadata logging to the new path.
- Sink instrumentation now logs `[LEVEL target]` when guard violations occur or
  when `trace_large_write` fires, giving us deterministic blame.
- `sink-userspace` propagates log metadata (level/target/topic) into the guard
  checker so `guard-str` lines describe exactly which log invocation supplied
  the bad slice.

## Current State (Phase 0)

This change introduces the `nexus-log` crate with the following minimal surface:

- `Level` enum and `set_max_level` helper backed by an `AtomicU8`.
- Builder-style API (`nexus_log::info/ warn / debug`) with a `LineBuilder`
  for hex/decimal/text emission without `fmt`.
- Two sinks behind features: `sink-userspace` uses `debug_putc`, `sink-kernel`
  uses the raw UART MMIO path borrowed from the trap logger.
- Topic-aware filtering: `Topic` bitmasks and `set_topic_mask` landed ahead of
  Phase 2 so verbosity scopes can be toggled without muting the entire log
  stream. `init-lite` uses the mechanism together with the
  `INIT_LITE_LOG_TOPICS` build-time env knob to gate noisy `svc-meta`
  instrumentation.
- The os-lite init path now uses the facade (via `nexus-init`), eliminating the
  previous custom formatter in `init-lite` that triggered `sepc=0` faults during bring-up.
- A pointer guard is pending integration to reject non-user strings before they
  hit the UART sink; the guard is tracked as part of the Phase 0 hardening work.

These changes replace only the logs we had to touch for ongoing debugging. The
rest of the system remains unchanged until we decide to adopt later phases. The loader/VMO provenance work called out in RFC‑0004 is a prerequisite for finishing Phase 0a; we are implementing the minimal subset (per-service metadata VMOs + W^X enforcement) in tandem with RFC‑0002 to keep service bring‑up safe.

With the move to a kernel `exec` loader, the userspace footprint shrinks to boot markers and policy hand-off. The existing topic gating (`INIT_LITE_LOG_TOPICS`, `probe`) stays for optional deep dives, but the default path aims to be quiet: kernel-side logging covers loader/mapping events, userspace only emits structured readiness markers.

### Interim Debugging Measures

- While Phase 0 is in effect we rely on extended instrumentation (UART probes,
  per-call-site guards) to pinpoint lingering pointer faults. These probes are
  temporary and will be removed once Phase 0a lands.
- The Phase 0 implementation still accepts raw `&str` arguments; we deliberately
  keep this to minimise churn during the current `init-lite` bring-up. Phase 0a
  will swap these for pre-validated `StrRef` handles and, where necessary, copy
  data into bounded bounce buffers before handing it to the sink. This mirrors
  the approach used in Fuchsia and seL4 and ensures a bad caller cannot corrupt
  the logging backend.

## Roadmap

| Phase | Scope | Notes |
| ----- | ----- | ----- |
| 0 | Shim + adoption in `init-lite` (this patch) | establishes crate, builder API, basic level gating |
| 0a | Safety hardening for Phase 0 | enable pointer guards in userspace sink, document string provenance expectations, introduce `StrRef`/handle API and bounded copy buffers so call sites cannot pass raw pointers |
| 1 | Kernel integration | expose feature `sink-kernel`, port `log_*` macros to delegate to `nexus-log`, keep raw panic helper for early boot |
| 2 | Runtime controls | per-target bitmask via syscall/`AtomicU32`, CLI (`logctl`) to toggle masks |
| 3 | Structured extensions | optional key/value helpers, ring buffer sink, userland consumer |

Each phase should keep panic paths allocator-free and must not delete existing
diagnostics until equivalent ones are in place.

## Open Questions

- How much metadata do we want to include in prefixes (hart id, timestamp)?
- Where should runtime configuration live (kernel global vs. capability-driven)?
- Do we eventually replace `debug_putc` syscall, or treat it as the sink backing
  `nexus-log`?

## Impact

- Introduces a new workspace crate (`source/libs/nexus-log`).
- Updates `init-lite` to depend on `nexus-log` and removes the ad-hoc `Logger`
  formatter that previously called into `core::fmt`.
- No other crates are modified yet; future phases will track their own PRs.

## Testing

- `cargo build -p nexus-log` (userspace feature) – ensures new crate builds.
- `cargo build -p init-lite --target riscv64imac-unknown-none-elf` – verifies
  the consumer compiles with the new API.
- Runtime validation is manual during the current debugging session; the new
  logs already emit through the existing UART path.*** End Patch
