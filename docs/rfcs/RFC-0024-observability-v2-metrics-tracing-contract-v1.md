# RFC-0024: Observability v2 local contract - metricsd + tracing export via logd

- Status: Draft
- Owners: @runtime
- Created: 2026-02-11
- Last Updated: 2026-02-11
- Links:
  - Tasks: `tasks/TASK-0014-observability-v2-metrics-tracing.md` (execution + proof)
  - ADRs: `docs/adr/0025-qemu-smoke-proof-gating.md` (deterministic QEMU proof policy)
  - Related RFCs:
    - `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`
    - `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`
    - `docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md`
    - `docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md`

## Status at a Glance

- **Phase 0a (logd sink hardening preflight)**: ⬜
- **Phase 0 (local v2 contract + service/lib shape)**: ⬜
- **Phase 1 (bounded security and failure semantics)**: ⬜
- **Phase 2 (proof gates + anti-drift sync)**: ⬜

Definition:

- "Complete" means the contract is defined and the proof gates are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - Local observability v2 contract: `metricsd` data model and tracing span end export behavior.
  - Deterministic and bounded update/export semantics for counters, gauges, histograms, and spans.
  - Minimal `logd` append-path hardening required as sink preflight for observability load (without changing RFC-0011 ownership scope).
  - Local export contract via `nexus-log` -> `logd` as the authoritative sink for v1.
  - Security and failure invariants for malformed, abusive, or oversized producer input.
- **This RFC does NOT own**:
  - Cross-node tracing context propagation/correlation (`TASK-0038` scope).
  - Remote scrape/query pipeline over DSoftBus (`TASK-0040` scope).
  - Lock profiling feature semantics (`TASK-0041` scope; may consume this sink).
  - UI/perf-specific tracer design (`TASK-0143` scope).
  - Kernel scheduler/policy behavior (no kernel changes in this RFC).
  - Replacing the base `logd` contract from `RFC-0011`; this RFC only adds hardening deltas needed by observability v2 load profile.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC is implemented by `tasks/TASK-0014-observability-v2-metrics-tracing.md`.

## Context

`TASK-0006` established `logd` as a bounded local sink and query surface. `TASK-0014` introduces the next local layer:

- metrics (`counter`, `gauge`, `histogram`) with bounded cardinality,
- tracing spans with deterministic IDs and bounded live-span state,
- export through structured log records so the sink path is shared and auditable.

The contract must remain deterministic, bounded, and local-only for v1 to avoid scope drift and fake-proof behavior.

## Goals

- Define a stable local contract for metrics and span export that is testable on host and in QEMU.
- Define deterministic reject behavior for malformed/oversized/rate-abusive inputs.
- Define bounded memory and rate controls so observability cannot become a resource-exhaustion path.
- Keep implementation userspace-only with no kernel ABI expansion.

## Non-Goals

- Full OpenTelemetry semantic and protocol compatibility in v1.
- Remote scrape/collection over network transports.
- Cross-node trace context propagation and correlation.
- Unbounded labels, unbounded spans, or unbounded payload fields.

## Constraints / invariants (hard requirements)

- **Determinism**:
  - markers and proofs are deterministic and bounded;
  - span/trace IDs are deterministic in OS builds (no RNG dependency required).
  - QEMU proofs run with modern virtio-mmio defaults as deterministic floor (legacy mode is debug-only).
- **No fake success**:
  - `metricsd: ready` and `SELFTEST: ... ok` only after real export/update behavior occurs.
- **Bounded resources**:
  - explicit caps for total series, per-metric series, live spans, labels per point, bytes per frame/record.
- **Ownership/type/concurrency floor**:
  - use explicit newtypes for stable contract values (for example metric keys, series/span/trace identifiers, bounded field wrappers),
  - preserve explicit ownership boundaries from decode -> in-memory registry -> sink export,
  - no new `unsafe impl Send/Sync` without documented safety argument and coverage,
  - define these boundaries early even where first implementation slices are still single-threaded.
- **Security floor**:
  - identity and policy decisions use authenticated kernel metadata (`sender_service_id`), never payload identity claims.
  - sensitive fields are denied/redacted by policy default.
  - reject behavior is explicit and auditable (`invalid_args`, `over_limit`, `rate_limited`).
- **Stubs policy**:
  - any host-only or debug path is labeled and must not claim production-equivalent success.

## Proposed design

### Contract / interface (normative)

- **Service boundary**:
  - producer -> `metricsd` via compact versioned byte frames for update operations,
  - `metricsd` -> `logd` via structured records through `nexus-log`.
- **Metric model**:
  - `Counter(u64)`: monotonic increment only.
  - `Gauge(i64)`: set/replace semantics.
  - `Histogram`: fixed bucket layout configured from task-owned defaults.
- **Span model**:
  - span start registers live span state with deterministic IDs,
  - span end emits structured export with duration/status/attributes.
- **Deterministic IDs**:
  - `span_id` derived from `(sender_service_id, monotonic_local_counter)`,
  - `trace_id` derived from deterministic local source (no RNG requirement for v1).
- **Error model (contract categories)**:
  - invalid frame/field/value -> `invalid_args`,
  - cap/rate exceed -> `over_limit` or `rate_limited`,
  - malformed/unsupported operation -> explicit reject (no silent drop as success).
- **Retention/gatekeeping**:
  - in-memory bounds are mandatory;
  - persistence/rollup slices use `TASK-0009` `/state` substrate and remain bounded.

### Type and concurrency boundary (normative)

- Contract-facing IDs and keys should use named newtypes instead of raw primitives to reduce cross-field confusion.
- Decode and validation happen before registry mutation; invalid wire values never mutate internal state.
- Shared registry/export paths keep explicit thread-safety boundaries; no implicit cross-thread ownership transfer contracts.
- This requirement applies from the first slice, even if specific wrappers are initially introduced ahead of broad call-site adoption.

### Phases / milestones (contract-level)

- **Phase 0a**: harden `logd` APPEND sink path for observability load (bounds, rate guards, identity binding, deterministic rejects).
- **Phase 0**: local service/lib contract for metrics + spans + logd export path.
- **Phase 1**: hardening of cardinality/rate/size limits and deterministic reject/audit semantics.
- **Phase 2**: proof sync and anti-drift handoff, with explicit separation from remote/cross-node follow-ups.

## Security considerations

### Threat model

- Malicious or buggy producers attempt high-cardinality label floods.
- Producers send oversized payloads or malformed frames to trigger parse/alloc pressure.
- Producers spoof identity through payload fields.
- Sensitive values leak into telemetry labels/attributes.

### Security invariants

- Identity/policy decisions use authenticated `sender_service_id` only.
- Inputs are bounded before parse/alloc and rejects are deterministic.
- Exported records do not contain prohibited secret-bearing keys/values.
- Observability paths must not create a second policy authority outside `policyd`.

### DON'T DO

- Do not trust payload service names/IDs for authorization.
- Do not accept unbounded label cardinality or unbounded span/field growth.
- Do not log credentials/tokens/secrets in telemetry payloads or errors.
- Do not emit ready/ok markers before real sink-path proof succeeds.

### Mitigations

- `logd` preflight hardening for APPEND (field bounds + per-sender budgets + identity binding).
- Per-metric and global series caps.
- Live-span caps and bounded attribute counts/lengths.
- Per-subject token-bucket rate budgets.
- Deterministic reject counters and hardening markers for each reject class.

### Open risks

- Final wire opcode/field vocabulary can drift if follow-up tasks bypass this contract.
- Secret-redaction policy quality depends on key taxonomy and must be tested with explicit negative cases.

## Failure model (normative)

- Malformed/unsupported frames reject deterministically (`invalid_args` class).
- Over-capacity/rate-abuse rejects deterministically (`over_limit`/`rate_limited` class).
- No implicit fallback to unbounded mode.
- No sink-path success claim if logd export fails; failure must be visible via explicit bounded error counters/markers.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p logd -- reject --nocapture
cd /home/jenning/open-nexus-OS && cargo test -p metricsd -- --nocapture
```

Host proof must include at least one validation test for newtype boundary decode/reject behavior and one concurrency-boundary test (for example compile-time/thread-safety assertions where appropriate).

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

QEMU proof should run with modern virtio-mmio defaults to keep marker behavior deterministic across reruns.

### Deterministic markers (if applicable)

- `logd: reject invalid_args`
- `logd: reject over_limit`
- `logd: reject rate_limited`
- `SELFTEST: logd hardening rejects ok`
- `metricsd: ready`
- `SELFTEST: metrics counters ok`
- `SELFTEST: metrics histograms ok`
- `SELFTEST: tracing spans ok`
- `SELFTEST: metrics security rejects ok`

### Feature-to-proof matrix (soll-first, normative)

- **Phase 0a logd hardening contract**
  - MUST be proven by reject-focused host tests and reject-focused QEMU markers.
  - MUST include a no-fake condition: invalid-only sink traffic does not emit metrics success markers.
- **Metrics contract (counter/gauge/histogram)**
  - MUST be proven with deterministic host vectors that assert desired semantics (not current implementation quirks).
  - QEMU success markers MUST be gated on sink-path evidence (records queryable via logd path), not only in-memory state.
- **Tracing contract (span lifecycle/export)**
  - MUST be proven by deterministic start/end lifecycle tests and sink-path span-end evidence checks.
  - Success markers MUST appear only after export visibility is confirmed.
- **Type/concurrency contract (newtypes + Send/Sync boundaries)**
  - MUST include boundary decode/reject tests for newtypes.
  - MUST include compile-time or equivalent thread-safety boundary checks when shared state is introduced.
- **Determinism contract (modern MMIO floor)**
  - QEMU proofs MUST run with modern virtio-mmio defaults and bounded timeouts.
  - Legacy mode remains debug/bisect only and is not acceptable as primary green proof.

## Alternatives considered

- Embed metrics/tracing directly into `logd`:
  - rejected to keep sink and aggregation responsibilities separated.
- Cap'n Proto as sole OS wire contract:
  - rejected for bring-up; byte frames remain authoritative in OS-lite v1.
- Random span/trace IDs:
  - rejected in v1 due to deterministic proof and no-RNG dependency floor.

## Open questions

- Should reject categories be mapped to a stable numeric status table in v1 or stay semantic labels until phase 1 closes?
- Which default histogram buckets are global vs service-specific in v1 config?
- Which attribute key policy (allowlist-first vs denylist-first) is preferred for initial rollout?

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

- [ ] **Phase 0a**: `logd` APPEND hardening (bounds/rate/identity) + deterministic reject proof - proof: `cargo test -p logd -- reject --nocapture`
- [ ] **Phase 0**: local metrics/spans contract + `metricsd`/`nexus-metrics` skeleton + sink-path proof wiring - proof: `cargo test -p metricsd -- --nocapture`
- [ ] **Phase 1**: cardinality/rate/size hardening + deterministic reject matrix - proof: `cargo test -p metricsd -- reject --nocapture`
- [ ] **Phase 2**: QEMU marker ladder + anti-drift sync with follow-up boundaries - proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
