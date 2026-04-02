# Handoff Archive: TASK-0019 ABI syscall guardrails v2

**Date**: 2026-03-27  
**Task**: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md`  
**Status**: `Done`  
**RFC**: `docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md` (`Complete`)

---

## Scope closed

- Kernel remained untouched.
- Userland ABI guardrails shipped as deterministic, bounded, deny-by-default checks.
- Profile distribution is authenticated (`sender_service_id`) with subject binding on kernel-derived identity.
- Lifecycle boundary stayed static (boot/startup apply only); runtime learn/enforce and kernel sysfilter remain follow-up scope.

## Proof closure

- Host:
  - `cargo test -p nexus-abi -- reject --nocapture`
  - `cargo test -p policyd abi_profile_get_v2 -- --nocapture`
- OS:
  - `just dep-gate`
  - `just diag-os`
  - `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- Additional full-gate verification:
  - `make build MODE=host`
  - `make test MODE=host`
  - `make run MODE=host RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s`

## Marker contract observed

- `abi-profile: ready (server=policyd|abi-filterd)`
- `abi-filter: deny (subject=<svc> syscall=<op>)`
- `SELFTEST: abi filter deny ok`
- `SELFTEST: abi filter allow ok`
- `SELFTEST: abi netbind deny ok`

## Follow-on boundaries

- `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
- `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`
