# ADR-0045: pinched — system-internal compute broker with exchangeable backends

## Status

Accepted (2026-07-17). Implemented and boot-proven (SMP=1 + SMP=2 marker
gates).

## Context

The SMP track (phases A–C) gave the OS real multi-core execution, a
scheduling ABI (affinity/shares) and `nexus-workpool`, the process-wide
deterministic compute pool. What was missing was a way for the SYSTEM to use
that parallelism on batch workloads without pushing the complexity onto app
developers — who, by explicit product decision, must never see or understand
parallelism.

We evaluated building directly on an interaction-net runtime for everything.
Rejected for v1: constant-factor overhead versus buffer workloads,
allocation-heavy evaluation on never-freeing bump allocators, and a central
IPC-service hotspot. The durable insight: keep the INTERFACE declarative and
stable, make the EVALUATOR exchangeable.

## Decision

`source/services/pinched/` is a system-internal broker for declarative
partition→map batch jobs:

- **Invisible to apps** (binding): no DSL surface, no `dsl_services.capnp`
  entry, no `nexus-sdk-routes` entry, no app permission. Only system services
  and SDK internals call it, wrapped in ordinary synchronous functions.
- **Latency doctrine** (binding): only latency-uncritical "user is waiting"
  batch paths (asset bakes, compiles, bulk transforms). Frame hotpaths are
  forbidden. Callers keep an inline fallback — fail open on LOCAL compute,
  never on waiting.
- **Wire**: an `OP_COMPUTE` frame names the job kind + partition domain; the
  data VMO travels via CAP_MOVE (RFC-0072 splice pattern, no frame copies).
  The service computes in place and completes by writing the VMO's 16-byte
  header LAST (release fence, `DONE_MAGIC`); the header carries status, an
  element count and `workers` — the honest dispatch counter (`0` = inline
  fallback). Oversized/malformed jobs are rejected through the header, never
  queued, never truncated.
- **Backends are exchangeable behind the same wire**: v1 executes on
  `nexus-workpool` (ADR-0046); the interaction-net evaluator `nexus-inet`
  (ADR-0047) is the second backend, proving the seam.

Job kinds as of this ADR: `JOB_MAP_MIX_U32` (proof transform),
`JOB_SVG_RASTER` (banded parallel SVG rasterization on the `nexus-svg`
`RasterPlan` band API), `JOB_INET_TREE_SUM` (net-evaluator workload).

## Consequences

- Proofs are result-proofs, not timing: identical bytes/values vs a
  host-pinned golden digest PLUS honest dispatch counters
  (`SELFTEST: pinched bounded/determinism/svg ok`,
  `SELFTEST: inet bounded/determinism/parallel exec ok`). QEMU wall-clock is
  deliberately not gated.
- Golden constants live next to a host test that regenerates them
  (`pinched::broker` + `proof_svg_digest_matches_pinned` etc.), so they
  cannot drift from the implementation.
- The service's bump allocator never frees; per-job parse/plan allocations
  accumulate. Acceptable for occasional batch jobs; a high-frequency caller
  needs an arena-reset follow-up first.
- One job at a time; backpressure is the blocked sender.

## How to use and extend

- **New job kind**: add the `JOB_*` constant + validation limits to
  `pinched::protocol` (the SSOT), a `handle_*` in `os_lite`/a sibling module
  (≤600 LOC per file), reject everything oversized via the header, report
  `workers` honestly, and add a result-proof marker (declared in the
  proof-manifest) whose expected value is host-pinned.
- **New backend**: implement it as a library crate (like `nexus-inet`),
  dispatch to it per job kind inside the service — the wire, the visibility
  and latency doctrines, and the header contract stay untouched.
- **Async completion**: the header has reserved space; a result-fence
  (client waits via waitset/fence instead of polling) is the planned
  extension for async system clients — add it as a flag, never break pollers.
- **Future scope** (explicitly out of v1): result cache (content-keyed,
  bounded), generic net-serialization wire format, GPU/distributed backends
  behind the same job-graph interface.

## References

- `docs/dev/app-platform/pinched-compute-broker.md` (doctrine + wire summary)
- ADR-0046 (workpool), ADR-0047 (nexus-inet), ADR-0048 (integrity detectors)
- RFC-0072 (VMO splice pattern)
