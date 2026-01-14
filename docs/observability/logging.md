## Logging (Observability v1)

This document describes the **v1 logging system** introduced by `TASK-0006` (complete as of 2026-01-14).

Primary contract references:

- `docs/rfcs/RFC-0003-unified-logging.md` (logging facade + marker discipline)
- `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (logd journal + crash report v1 seed; **Complete**)
- `scripts/qemu-test.sh` (authoritative marker contract)
- `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md` (execution truth)

### Components

- **`logd` (OS service)**: bounded in-RAM journal for structured records (drop-oldest on overflow).
- **`nexus-log` (facade)**: the unified API surface for services to emit logs.
- **`execd` (OS service)**: produces crash report markers and appends a crash event into `logd` on non-zero exits.

### Bounded journal (RAM ring)

`logd` stores log records in a bounded journal:

- **Capacity is fixed** (records + bytes).
- **Overflow policy**: drop the **oldest** records until the new record fits.
- **Per-field bounds (v1)**:
  - `scope_len <= 64` bytes
  - `message_len <= 256` bytes
  - `fields_len <= 512` bytes (opaque payload in v1)

These bounds are enforced on the **wire** and must not allocate unbounded memory.

### OS wire protocol (v1)

`logd` exposes a small byte-frame protocol (APPEND / QUERY / STATS).

For the on-wire contract and response status codes, see:

- `docs/rfcs/RFC-0011-logd-journal-crash-v1.md`

### UART markers and “no fake green”

Markers are part of the test contract.

- Markers must be **deterministic** (stable strings; no timestamps/randomness).
- Markers must be **honest**:
  - `logd: ready` is emitted only once the IPC endpoint is live.
  - `SELFTEST: … ok` is emitted only after real behavior (not log-grep optimism).

### Crash reports (v1)

When a supervised process exits non-zero, `execd` emits:

- UART marker: `execd: crash report pid=<pid> code=<code> name=<name>`
- An appended record into `logd` (scope `execd`, bounded message) so selftests can verify crash reporting **without scraping UART**.

### Core service integration (v1)

The following core services emit structured logs to `logd` via `nexus-log`:

- `samgrd` (service registry)
- `bundlemgrd` (bundle manager)
- `policyd` (policy enforcement; decision audit only, never secrets)
- `dsoftbusd` (distributed bus)

Existing UART readiness markers (`*: ready`) are preserved for deterministic QEMU testing.
`selftest-client` validates the integration via bounded `QUERY` (time-windowed) and `STATS` delta proof.

### Proof gates (as of 2026-01-14)

**Host**:

```bash
cargo test -p logd -- --nocapture
cargo test -p nexus-log -- --nocapture
```

**OS/QEMU**:

```bash
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

**Required markers** (all present and green):

- `logd: ready`
- `SELFTEST: log query ok`
- `SELFTEST: core services log ok`
- `execd: crash report pid=... code=42 name=demo.exit42`
- `SELFTEST: crash report ok`

### Future direction (v2+)

See `tasks/TASK-0014-observability-v2-metrics-tracing.md` for planned extensions:

- Persistent journal (statefs/blk-backed) with rotation
- Subscriptions / streaming (log consumers) with backpressure
- Runtime filters (`logctl`) and structured field schema evolution
- Export/bridging (remote collector via `dsoftbusd`)
- Tracing/metrics integration
