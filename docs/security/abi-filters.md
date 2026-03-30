# ABI syscall guardrails (TASK-0019)

## Scope and boundary

ABI syscall filters in TASK-0019 are a **userspace guardrail** for compliant
binaries. They are **not** a hard sandbox against malicious code issuing raw
`ecall` instructions.

- Kernel remains unchanged in this slice.
- True raw-ecall enforcement is deferred to kernel scope (`TASK-0188`).

## Security invariants

- Profile distribution is accepted only from authenticated policy authority
  identity (`sender_service_id` must match `policyd` service id).
- Profile subject binding is kernel-derived (`service_id`), never payload text.
- Profile decode/matching is bounded:
  - max profile bytes,
  - max rule count,
  - max path-prefix/path sizes,
  - bounded matcher cost.
- Decision model is deny-by-default and deterministic.

## Lifecycle (TASK-0019 only)

- Static lifecycle only:
  - profile fetch/apply at startup,
  - no runtime learn/enforce switching,
  - no runtime hot reload.
- Runtime lifecycle evolution belongs to `TASK-0028`.

## Marker contract

- `abi-profile: ready (server=policyd|abi-filterd)`
- `abi-filter: deny (subject=<svc> syscall=<op>)`
- `SELFTEST: abi filter deny ok`
- `SELFTEST: abi filter allow ok`
- `SELFTEST: abi netbind deny ok`

## Required negative host proofs

- `test_reject_unbounded_profile`
- `test_reject_unauthenticated_profile_distribution`
- `test_reject_subject_spoofed_profile_identity`
- `test_reject_profile_rule_count_overflow`

## Deterministic hardening proofs (additional)

- Matcher and fail-closed behavior:
  - `test_reject_first_match_precedence_conflict_is_deterministic`
  - `test_reject_trailing_profile_bytes_as_malformed`
  - `test_reject_statefs_put_oversized_payload_fail_closed`
  - `test_reject_typed_distribution_subject_mismatch`
- policyd distribution frame handling:
  - `test_abi_profile_get_v2_malformed_frame_is_fail_closed`
  - `test_abi_profile_get_v2_allows_privileged_proxy_subject_mismatch`

## Anti-fake-green note

- Selftest marker emission is behavior-coupled:
  - ABI `ok` markers are emitted only after profile subject binding matches the
    local kernel-derived `selftest-client` service identity and policy decisions
    are verified.
