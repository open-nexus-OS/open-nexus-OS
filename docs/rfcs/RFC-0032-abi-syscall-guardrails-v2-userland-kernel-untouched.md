# RFC-0032: ABI syscall guardrails v2 (userland, kernel-untouched)

- Status: In Progress
- Owners: @runtime @security
- Created: 2026-03-26
- Last Updated: 2026-03-26
- Links:
  - Tasks: `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` (execution + proof)
  - Follow-on tasks:
    - `tasks/TASK-0028-abi-filters-v2-arg-match-learn-enforce.md`
    - `tasks/TASK-0188-kernel-sysfilter-v1-task-profiles-rate-buckets.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
    - `docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md`
    - `docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md`

## Status at a Glance

- **Phase 0 (contract freeze + phased rollout plan)**: 🟨
- **Phase 1 (bounded filter chain + deterministic deny/audit)**: ⬜
- **Phase 2 (authenticated profile distribution)**: ⬜
- **Phase 3 (QEMU markers + rollout closure)**: ⬜

Definition:

- "Complete" means the contract is defined and proof gates are green (host tests + marker gates). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - kernel-unchanged ABI syscall guardrail contract in userspace,
  - deterministic deny-by-default matcher behavior for compliant wrapper callers,
  - authenticated profile distribution contract and subject-binding rules,
  - marker semantics for deny/allow/netbind-deny proof closure.
- **This RFC does NOT own**:
  - kernel-enforced syscall sandboxing (`TASK-0188`),
  - runtime `learn/enforce` lifecycle transitions and policy generation (`TASK-0028`),
  - unrelated crashdump/pipeline follow-on scope.

### Relationship to tasks (single execution truth)

- `tasks/TASK-0019-security-v2-userland-abi-syscall-filters.md` defines stop conditions and proof commands.
- `TASK-0019` remains static-lifecycle (boot/startup apply) by contract.
- Runtime lifecycle transitions are contract-owned by `TASK-0028`.

## Context

Open Nexus OS needs seccomp-like syscall hygiene before kernel sysfilter enforcement exists. Today, malicious code can bypass wrapper checks via raw `ecall`, so v2 must be explicit: this is defense-in-depth for compliant binaries, not a hard sandbox. The immediate value is deterministic deny behavior, auditable decisions, and phased migration toward wrapper-only syscall entry across shipped OS components.

## Goals

- Define a phased, deterministic userland syscall guardrail contract with kernel untouched.
- Enforce authenticated profile source + subject binding (`sender_service_id` and service identity).
- Prove deny/allow behavior and audit emission with stable markers and bounded parsing.

## Non-Goals

- Kernel-level syscall enforcement against raw `ecall` bypasses.
- Runtime lifecycle transitions (`learn/enforce`, hot reload) in this RFC/task slice.
- Full policy-generation workflow and regex-heavy argument policy evolution.

## Constraints / invariants (hard requirements)

- **Kernel untouched**: no changes in `source/kernel/**`.
- **No fake success**: success markers only after real allow/deny behavior and proof commands pass.
- **Bounded resources**: bounded profile/rule counts, bounded decode, bounded matcher cost, bounded audit emission.
- **Determinism**: stable reject labels/errors/markers; no timing-flake gates.
- **Security floor**:
  - deny-by-default profile semantics,
  - authenticated profile authority path,
  - subject identity sourced from kernel-derived IDs, never payload strings.

## Proposed design

### Contract / interface (normative)

- **Filter path**: `nexus-abi` wrappers run a pre-ecall policy check for covered syscall classes.
- **Decision model**: allow or deny with stable `AbiError`/`errno` mapping; deny emits audit evidence.
- **Profile model**: deterministic bounded profile format (v1), default deny, first-match-wins.
- **Identity model**: profile distribution requests are accepted only from authenticated authority (`sender_service_id`) and must bind to canonical subject identity.
- **Marker model**:
  - `abi-profile: ready (server=policyd|abi-filterd)`
  - `abi-filter: deny (subject=<svc> syscall=<op>)`
  - `SELFTEST: abi filter deny ok`
  - `SELFTEST: abi filter allow ok`
  - `SELFTEST: abi netbind deny ok`

### Phases / milestones (contract-level)

- **Phase 0**: finalize bounded contract + phased rollout boundaries.
- **Phase 1**: implement minimal bounded filter chain + deterministic deny/audit path for selected syscall subset.
- **Phase 2**: add authenticated profile distribution path and subject-binding rejects.
- **Phase 3**: complete staged rollout evidence and QEMU marker closure.

### Lifecycle boundary (normative)

- This RFC slice remains static lifecycle only:
  - profile apply at startup/boot-time,
  - no runtime mode-switch API,
  - no hot-reload contract.
- Runtime lifecycle transitions are deferred by contract to `RFC/TASK-0028` scope.

## Security considerations

- **Threat model**:
  - raw `ecall` bypass by malicious code,
  - unauthenticated profile injection/tampering,
  - subject spoofing through payload identity,
  - audit flooding / parser resource abuse.
- **Security invariants**:
  - deny-by-default matching for covered syscall path,
  - authenticated authority for profile distribution,
  - subject-binding through kernel-derived IDs,
  - deterministic bounded reject behavior for malformed/oversized profile input.
- **DON'T DO**:
  - don't claim sandbox-level enforcement in this RFC,
  - don't accept profile authority from payload text,
  - don't add unbounded rule parsing or unbounded audit spam,
  - don't add runtime lifecycle transitions in this slice.
- **Proof strategy**:
  - required negative tests for unbounded profile, unauthenticated profile path, spoofed subject, rule overflow,
  - deterministic marker ladder in QEMU for deny/allow/netbind deny.
- **Open risks**:
  - partial rollout phases can temporarily leave non-migrated call paths outside guardrail coverage,
  - final server choice (`policyd` vs `abi-filterd`) must be locked before marker contract freeze.

## Failure model (normative)

- Invalid profile payloads fail-closed with deterministic errors.
- Unauthenticated or subject-mismatched distribution requests fail-closed.
- When profile data is unavailable or invalid, system behavior is deny-by-default for covered operations.
- No silent fallback from authenticated profile distribution to unauthenticated local overrides.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-abi -- reject --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers (if applicable)

- `abi-profile: ready (server=policyd|abi-filterd)`
- `SELFTEST: abi filter deny ok`
- `SELFTEST: abi filter allow ok`
- `SELFTEST: abi netbind deny ok`

## Alternatives considered

- **Immediate kernel sysfilter work in this slice**: rejected; belongs to `TASK-0188` and needs separate kernel contract/proofs.
- **Single big-bang migration for all OS components**: rejected; too risky and hard to prove deterministically in one slice.
- **`abi-filterd` as mandatory new service**: rejected for now; `policyd` remains preferred authority unless coupling proves untenable.

## Open questions

- Should the first rollout phase include only statefs + net-bind classes, or one additional syscall class for earlier migration coverage?
- Do we require `policyd`-only in v1, or keep `abi-filterd` as equally supported fallback in the final contract text?

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- scope boundaries remain explicit and linked to `TASK-0019` (execution) and `TASK-0028`/`TASK-0188` (follow-on ownership),
- deterministic and bounded behavior is preserved in both matcher and distribution paths,
- security invariants and negative tests remain explicit and current,
- proof strategy reflects canonical commands and real marker contracts only.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [ ] **Phase 0**: Contract freeze + phased rollout boundaries — proof: task/header + RFC sync complete.
- [ ] **Phase 1**: Bounded filter chain + deterministic deny/audit path — proof: `cargo test -p nexus-abi -- reject --nocapture`
- [ ] **Phase 2**: Authenticated profile distribution + subject-binding rejects — proof: required `test_reject_*` coverage green.
- [ ] **Phase 3**: QEMU marker closure and rollout evidence — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`).
