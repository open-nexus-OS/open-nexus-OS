<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-0011: logd journal + crash reports v1

- Status: Complete
- Owners: @runtime
- Created: 2026-01-13
- Last Updated: 2026-01-14
- Proof Gates: Green (QEMU markers present, host tests pass)
- Links:
  - Tasks: `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0003-unified-logging.md`
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
    - `docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md`
  - ADRs:
    - `docs/adr/0017-service-architecture.md`
  - IDL schema (host/control plane doc): `tools/nexus-idl/schemas/log.capnp`
  - QEMU marker contract: `scripts/qemu-test.sh`

## Status at a Glance

- **Phase 0 (logd OS contract + bounded journal)**: Complete
- **Phase 1 (nexus-log logd sink)**: Complete
- **Phase 2 (execd crash reports using logd)**: Complete
- **Phase 3 (core service wiring proof)**: Complete

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers).
  It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The v1 **OS on-wire contract** for logd (byte frames: APPEND/QUERY/STATS).
  - The v1 **bounded RAM journal** semantics (drop-oldest, counters).
  - The v1 **crash report envelope** semantics (minimal, bounded; no dumps).
  - Determinism + “no fake success” marker rules for these components.
- **This RFC does NOT own**:
  - Kernel changes (no kernel ring buffer, no new syscalls).
  - Persistent journaling / log rotation.
  - Remote log export / subscriptions / streaming.
  - Bulk log transfer via VMO/filebuffer (deferred; see follow-up tasks).
  - Policy surface (`logctl`) and audit/export formats (deferred).

### Relationship to tasks (single execution truth)

- Tasks define **stop conditions** and **proof commands**.
- This RFC must remain a “100% done” seed. Future behavior upgrades must land in new RFC(s) if
  scope expands.

## Context

Current bring-up uses UART prints/markers and `nexus-log` facade (RFC-0003), but there is no OS journaling service.
We need **bounded, deterministic** log collection in userspace, and minimal crash reporting derived from recent logs,
without touching the kernel.

## Goals

- Provide `logd` as an OS service that:
  - stores structured log records in a **bounded RAM journal**,
  - enforces **size bounds** and **drop-oldest** overflow behavior,
  - returns deterministic stats, and
  - emits `logd: ready` **only after the IPC endpoint is live**.
- Provide a minimal crash report flow where `execd` can:
  - observe non-zero exits, and
  - emit a deterministic crash marker + logd event derived from recent journal entries (bounded).
- Provide a minimal “core service wiring” proof:
  - selected core services emit at least one structured record to logd via `nexus-log`,
  - existing UART readiness markers remain unchanged (no marker drift),
  - selftest validates the path via bounded `QUERY` (not by UART scraping).

## Non-Goals

- Persistent journal to disk/statefs.
- Full tracing/metrics export (v2+).
- Kernel log integration / kernel-side ring buffers.
- Unbounded or streaming query results.

## Constraints / invariants (hard requirements)

- **Kernel unchanged**.
- **Determinism**:
  - Marker strings are stable and non-random.
  - Tests are bounded (no infinite loops; bounded retries only).
- **No fake success**:
  - `logd: ready` only after endpoints are live.
  - `SELFTEST: ... ok` only after real behavior.
- **Bounded resources**:
  - Per-record size caps.
  - Fixed journal capacity (records + bytes).
  - Query responses bounded by `maxCount` and response byte cap.
- **Security floor**:
  - Never trust caller-provided identity. Use kernel IPC `sender_service_id`.
  - Never log secrets (keys, credentials).
  - Reject malformed/oversized frames deterministically (no panic).
- **Stubs policy**:
  - Any stub must return `Unsupported` / emit `stub` markers; never claim “ok/ready”.

## Follow-up compatibility (explicit; prevents hidden prerequisites)

This RFC is intentionally v1-minimal, but follow-up tasks rely on a few *stable* properties of logd.
To avoid hidden assumptions and drift, the following are part of the v1 contract:

- **Structured event bus (bounded, in-RAM)**:
  - logd stores structured records and supports bounded `QUERY` so other services can validate exports by reading back recent records.
  - logd does **not** provide streaming/subscriptions or persistence in v1.
- **Authenticated origin**:
  - Every stored record includes `service_id = sender_service_id` from kernel IPC metadata (unforgeable).
  - Callers MUST NOT be able to spoof origin via payload strings.
- **Opaque `fields` blob reserved for follow-ups**:
  - `fields` is a bounded opaque byte blob (v1 does not interpret it).
  - Follow-up tasks may define stable encodings within this blob (e.g. metrics snapshots, span-end events, crash metadata),
    but MUST remain within the v1 bounds and MUST be deterministic.
- **Crash metadata extensibility (no kernel changes)**:
  - v1 crash reporting emits a minimal crash event derived from recent logs.
  - Later tasks (e.g. crashdumps/export) can extend crash events to include `build_id` and `/state` artifact paths without changing logd wire framing.

## Proposed design

### Contract / interface (normative)

#### Transport model

- **OS / os-lite backend (authoritative for v1)**: versioned byte frames over kernel IPC.
- **Host / std backend**: Cap’n Proto for typed tests and evolution tracking (not authoritative for OS wire format).

#### Magic + versioning

All logd v1 frames begin with:

```text
[MAGIC0, MAGIC1, VERSION, OP, ...payload]
```

- `MAGIC0` = `b'L'` (0x4C)
- `MAGIC1` = `b'O'` (0x4F)
- `VERSION` = `1`
- `OP`:
  - `APPEND = 1`
  - `QUERY  = 2`
  - `STATS  = 3`

Responses MUST set the high bit on the opcode:

```text
OP_RSP = OP | 0x80
```

#### Bounded inputs (hard caps)

All v1 parsing must enforce:

- `scope_len <= 64` bytes
- `message_len <= 256` bytes
- `fields_len <= 512` bytes (opaque, e.g. JSON/CBOR; not interpreted by logd v1)
- `maxCount <= 16` (hard cap)
- `query_response_bytes <= 2048` (hard cap; additional records are truncated)

#### Status codes (v1)

All response frames include a `status` byte:

- `STATUS_OK = 0`
- `STATUS_MALFORMED = 1`
- `STATUS_UNSUPPORTED = 2`
- `STATUS_TOO_LARGE = 3`

#### APPEND (v1)

Request payload:

```text
[level:u8,
 scope_len:u8,
 msg_len:u16le,
 fields_len:u16le,
 scope[scope_len],
 msg[msg_len],
 fields[fields_len]]
```

Response payload:

```text
[status:u8,
 record_id:u64le,
 dropped:u64le]
```

Notes:

- `record_id` is monotonic (starts at 1); `0` on error.
- `dropped` is the total number of dropped records due to journal overflow.

#### QUERY (v1)

Request payload:

```text
[since_nsec:u64le, max_count:u16le]
```

Response payload:

```text
[status:u8, count:u16le, records..., total:u64le, dropped:u64le]
```

Where each record is encoded as:

```text
[record_id:u64le,
 timestamp_nsec:u64le,
 service_id:u64le,
 level:u8,
 scope_len:u8,
 msg_len:u16le,
 fields_len:u16le,
 scope[scope_len],
 msg[msg_len],
 fields[fields_len]]
```

Notes:

- Records are returned in ascending journal order.
- `since_nsec` is best-effort: if clocks are coarse or unavailable, zero timestamps may cause “everything since 0” behavior.

#### STATS (v1)

Request payload: empty.

Response payload:

```text
[status:u8,
 total:u64le,
 dropped:u64le,
 capacity_records:u32le,
 capacity_bytes:u32le,
 used_records:u32le,
 used_bytes:u32le]
```

### Journal semantics (normative)

- The journal is bounded by **both** record count and byte size.
- On overflow, logd MUST drop the **oldest** records until the new record fits.
- Counters:
  - `total`: total successfully appended records since boot.
  - `dropped`: total dropped due to overflow since boot.
- Identity binding:
  - Each stored record MUST store `service_id = sender_service_id` from kernel IPC metadata.
  - Callers MUST NOT be able to spoof `service_id`.

### Crash reports (v1 minimal)

Crash reporting is a userspace behavior contract:

- When a supervised process exits with a non-zero code, `execd` emits:
  - Marker: `execd: crash report pid=<pid> code=<code> name=<name>`
  - A structured event to logd (bounded) containing:
    - service name (or bundle name)
    - pid
    - exit code
    - timestamp (best-effort)
    - recent logs (bounded: last N / last window)

#### Crash event fields (v1 keys; stable)

To minimize drift across crash-related follow-ups (TASK-0018/TASK-0141), `execd` SHOULD emit a crash record using:

- `scope = "execd"`
- `message` containing a short, human-readable summary (bounded)
- `fields` encoded as deterministic key/value pairs (encoding below) with these keys:
  - `event=crash.v1`
  - `pid=<u32>`
  - `code=<i64>`
  - `name=<utf8 bounded>`
  - `recent_window_nsec=<u64>` (best-effort)
  - `recent_count=<u16>` (how many journal records were summarized/considered)
  - Reserved for future tasks (not required in v1): `build_id`, `dump_path`

This is intentionally minimal: follow-up tasks may add keys, but MUST keep existing keys stable.

#### `fields` encoding (deterministic, bounded; v1 default convention)

For v1 interoperability without introducing a schema dependency, the default convention for `fields` is:

- UTF-8 bytes containing `key=value` pairs separated by `\n`
- keys are ASCII `[a-z0-9_.-]+`
- values are UTF-8 and MUST NOT contain `\n`
- pairs SHOULD be sorted lexicographically by key for determinism

logd v1 does not parse this; it is a convention used by producers/consumers.

Non-goals for crash v1:

- No memory dump, no stack dump, no symbolization (deferred to TASK-0018 / TASK-0141).

## Security considerations

### Threat model

- **Flooding / DoS**: attacker spams APPEND to exhaust memory.
- **Log injection / spoofing**: attacker attempts to forge identity/service names.
- **Information disclosure**: secrets accidentally logged and later queried/exported.
- **Crash report leakage**: crash reports include sensitive content.

### Mitigations / invariants

- Enforce strict size bounds before allocation (reject oversized input).
- Journal is bounded and uses drop-oldest (never OOM via logs).
- `service_id` is always kernel-derived `sender_service_id`.
- No secret material must be logged; services treat logs as potentially exportable.
- Crash report is a bounded envelope; no raw memory.

### Open risks (explicit)

- Query access control is minimal in v1. Capability distribution and future `policyd` integration (TASK-0014 / follow-up) must enforce least privilege.

## Failure model (normative)

- Malformed frames MUST return `STATUS_MALFORMED` (best-effort response shape).
- Unsupported version/op MUST return `STATUS_UNSUPPORTED`.
- Oversized inputs MUST return `STATUS_TOO_LARGE`.
- No silent fallback: if logd is unavailable, callers must not claim “logged ok”.

## Proof / validation strategy (required)

Tasks must implement the proofs below.

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p logd -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers (v1, required)

- `logd: ready`
- `SELFTEST: log query ok`
- `SELFTEST: core services log ok`
- `execd: crash report pid=<pid> code=<code> name=<name>`
- `SELFTEST: crash report ok`

## Alternatives considered

- **Cap’n Proto on OS wire**: rejected for v1 because `os-lite` bring-up needs minimal overhead
  and existing services use byte-frame OS contracts.
- **VMO/filebuffer bulk query in v1**: rejected as premature complexity; deferred until remote observability requires it.

## Open questions

- Query filtering/ownership: in v1 we rely on capability distribution; phase-1 hardening should add policy-gated query semantics.
- UART mirror format: JSONL vs plain text. Deferred unless needed for bring-up/export.

## Checklist (keep current)

- [x] Scope boundaries are explicit; cross-RFC ownership is linked.
- [x] Task exists and contains stop conditions + proof commands.
- [x] Proof is "honest green" (markers/tests), not log-grep optimism.
- [x] Determinism + bounded resources are specified.
- [x] Security invariants are stated and have at least one regression proof (bounds + identity binding).
- [x] Deterministic markers are implemented in OS/QEMU harness.
- [x] Stubs: N/A (no stubs in this RFC; all paths are real behavior).
