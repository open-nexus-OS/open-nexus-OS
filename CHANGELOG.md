# Changelog

All notable changes to Open Nexus OS will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added - 2026-01-14

#### Observability v1 (TASK-0006: Complete)

**New Services**:
- `logd`: Bounded RAM journal for structured logs
  - Wire protocol v1: APPEND/QUERY/STATS (versioned byte frames for OS, Cap'n Proto for host)
  - Ring buffer semantics: drop-oldest on overflow, deterministic counters
  - Authenticated origin: `sender_service_id` from kernel IPC metadata
  - RFC: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (Complete)

**Logging Integration**:
- `nexus-log` extended with `logd` sink (`sink-logd` feature)
- Core services integrated: `samgrd`, `bundlemgrd`, `policyd`, `dsoftbusd`
- Existing UART readiness markers preserved for deterministic testing
- Fallback: UART-only if `logd` unavailable

**Crash Reporting**:
- `execd` crash reporting for non-zero exits
  - UART marker: `execd: crash report pid=<pid> code=<code> name=<name>`
  - Structured crash event appended to `logd` (queryable for post-mortem)
  - Stable crash event keys: `event=crash.v1`, `pid`, `code`, `name`, `recent_count`
  - Reserved keys for future: `build_id`, `dump_path`

**Testing**:
- Host tests: `cargo test -p logd`, `cargo test -p nexus-log`
- QEMU markers (all green as of 2026-01-14):
  - `logd: ready`
  - `SELFTEST: log query ok`
  - `SELFTEST: core services log ok`
  - `execd: crash report pid=... code=42 name=demo.exit42`
  - `SELFTEST: crash report ok`

**Documentation**:
- New: `docs/observability/logging.md` (usage guide)
- New: `docs/rfcs/RFC-0011-logd-journal-crash-v1.md` (contract seed)
- Updated: `docs/architecture/` (10+ files), `docs/testing/index.md`, ADR-0017

**Demo Payloads**:
- `demo.exit42` added to `userspace/apps/demo-exit0` for crash report testing

**Breaking Changes**: None (additive only)

**Known Limitations (v1 scope)**:
- Journal is RAM-only (no persistence)
- No streaming/subscriptions (bounded queries only)
- No remote export (deferred to TASK-0040)
- No metrics/tracing integration (deferred to TASK-0014)

---

## [Previous Releases]

See Git history for releases prior to 2026-01-14.
