# RFC-0029: Netstackd modular daemon structure v1

- Status: Complete
- Owners: @runtime
- Created: 2026-03-24
- Last Updated: 2026-03-24
- Links:
  - Tasks: `tasks/TASK-0016B-netstackd-refactor-v1-modular-os-daemon-structure.md` (execution + proof)
  - ADRs: `docs/adr/0005-dsoftbus-architecture.md`
  - ADRs: `docs/adr/0026-network-address-profiles-and-validation.md`
  - Architecture SSOT: `docs/architecture/network-address-matrix.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0006-userspace-networking-v1.md`
    - `docs/rfcs/RFC-0017-device-mmio-access-model-v1.md`

## Status at a Glance

- **Phase 0 (module boundary definition)**: ✅
- **Phase 1 (behavior-preserving extraction)**: ✅
- **Phase 2 (loop and idiom hardening + proof sync)**: ✅

Definition:

- “Complete” means the contract is defined and the proof gates are green (tests/markers). It does not mean “never changes again”.

Post-completion sync (2026-03-24):

- Address/profile governance for QEMU + os2vm is now anchored outside this RFC in:
  - `docs/architecture/network-address-matrix.md`
  - `docs/adr/0026-network-address-profiles-and-validation.md`
- Runtime code and proof gates were re-validated after this governance sync:
  - `cargo test -p netstackd --tests -- --nocapture`
  - `cargo test -p dsoftbusd --tests -- --nocapture`
  - `just test-os-dhcp-strict`
  - `RUN_OS2VM=1 RUN_TIMEOUT=180s OS2VM_PROFILE=ci RUN_PHASE=end tools/os2vm.sh`

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the internal architectural boundaries for `source/services/netstackd/` during the refactor
  - the rule that bootstrap, IPC wire handling, handle bookkeeping, loopback shims, facade operations, and observability become explicit internal seams
  - the compatibility floor that this refactor must preserve existing networking-owner behavior, marker semantics, and IPC wire formats
  - the boundary between behavior-preserving structural work and later networking feature work
- **This RFC does NOT own**:
  - new public networking features or new IPC/wire contracts
  - MMIO capability design or distribution changes (owned by `RFC-0017` / `TASK-0010`)
  - changes to the userspace networking facade contract owned by `RFC-0006`
  - execution checklists, touched-path allowlists, or proof commands beyond what the task defines

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC links only the contract for the `netstackd` internal refactor; `TASK-0016B` remains the execution single source of truth.

## Context

`source/services/netstackd/src/main.rs` currently contains most of the OS daemon behavior in one file:

- entry/wiring,
- bootstrap + MMIO bring-up retry,
- DHCP/fallback/static IPv4 policy,
- gateway ping / UDP DNS / TCP listen selftests,
- wire parsing and reply frame encoding,
- listener/stream/UDP handle bookkeeping,
- local loopback shims for deterministic bring-up,
- the main service loop and operation dispatch.

This concentration raises review risk and makes follow-on networking tasks expensive because any change reopens the full daemon instead of one seam. It also keeps some state/loop ownership and reply-encoding behavior implicit instead of typed and locally testable. We need an explicit internal architecture for `netstackd` before adding more networking behavior.

## Goals

- Define a stable internal module boundary for `netstackd` that separates bootstrap, IPC wire handling, loopback support, facade operations, and observability responsibilities.
- Preserve the externally visible behavior of the current daemon while refactoring.
- Make later networking work able to reuse clear seams instead of editing a monolithic `main.rs`.
- Harden remaining large loops and typed ownership boundaries using modern Rust idioms without changing marker or wire contracts.

## Non-Goals

- Defining a new public ABI or new network wire version.
- Changing the networking ownership model defined by `RFC-0006`.
- Introducing new services, new markers, or new network capabilities.
- Moving this logic into shared core libraries in this RFC.

## Constraints / invariants (hard requirements)

- **Determinism**: marker ordering, retry budgets, and bounded loops must remain deterministic.
- **No fake success**: no `ok/ready` marker semantics may change as part of the refactor.
- **Bounded resources**: extraction must preserve explicit caps/budgets for loopback buffers, handle tables, and would-block retries.
- **Security floor**:
  - malformed IPC frames remain rejected deterministically,
  - `netstackd` remains the networking owner rather than exposing direct MMIO authority elsewhere,
  - logs do not leak secrets or nondeterministic content.
- **Stubs policy**: refactor may not replace real behavior with placeholder abstractions that still claim success.
- **No silent contract drift**: IPC frame layouts and proof marker names stay unchanged in this RFC.

## Proposed design

### Contract / interface (normative)

This RFC defines the internal architecture contract for `netstackd` v1:

- `main.rs` becomes a thin environment/entry wrapper.
- OS daemon responsibilities are split into explicit internal modules, with at least these seams:
  - runtime entry / top-level orchestration,
  - bootstrap/fallback configuration,
  - IPC wire constants, parsing, and reply encoding,
  - typed handle IDs and local state/context ownership,
  - loopback transport shim,
  - op-specific facade logic,
  - observability helpers.
- Orchestration flattening is mandatory for maintainability/debuggability:
  - long setup/retry/control paths are represented as explicit step helpers under `src/os/**`,
  - `main.rs` remains a readable control shell instead of carrying full execution blocks.
- Phase-1 hardening is behavior-preserving:
  - same wire formats,
  - same marker names,
  - same single-VM proof behavior,
  - same ownership contract from `RFC-0006`.

Versioning strategy:

- This is an internal daemon-structure contract, not a public wire/ABI version.
- If a later task changes public networking behavior, that follow-on work must create or update a dedicated RFC rather than silently expanding this one.

### Phases / milestones (contract-level)

- **Phase 0**: Define and land the internal module boundary for `netstackd` without changing proof behavior.
- **Phase 1**: Extract existing logic into those seams while preserving marker and wire semantics.
- **Phase 2**: Harden remaining explicit loops and typed ownership boundaries using newtypes, reply/parse helpers, `#[must_use]`, and host tests, while preserving existing proofs.

## Security considerations

- **Threat model**:
  - refactor regressions weaken malformed-frame rejection or handle/state validation
  - retry/loop cleanup hides liveness failures behind generic helpers
  - ownership boundaries become less explicit and invite future authority drift
- **Mitigations**:
  - preserve existing marker and wire proofs
  - add narrow tests for parse/reply helpers, typed IDs, and bounded state transitions
  - keep terminal failure policy explicit where the current daemon intentionally halts
- **Open risks**:
  - the current file mixes policy and mechanics enough that the best final module cut may only become fully clear during extraction
  - some bring-up-specific debug markers may need follow-up cleanup after the refactor if they do not fit the final observability seam

## Failure model (normative)

- If the refactor changes marker names, marker meaning, wire layouts, or retry semantics, the refactor is considered a failure for this RFC.
- If fallback behavior exists during extraction, it must remain explicit and behavior-equivalent; no silent fallback that changes proof semantics is allowed.
- If the refactor reveals a missing external contract, work must stop and either:
  - narrow the task back to the existing contract, or
  - create a new RFC/ADR for the newly discovered boundary.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p netstackd --tests -- --nocapture
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh
```

### Deterministic markers (if applicable)

- `netstackd: ready`
- `SELFTEST: net iface ok`
- `SELFTEST: net ping ok`
- `SELFTEST: net udp dns ok`
- `SELFTEST: net tcp listen ok`

## Alternatives considered

- Keep `main.rs` monolithic and defer cleanup until a feature task.
  - Rejected because every upcoming networking change would continue to reopen the whole daemon and increase drift risk.
- Extract directly into shared networking crates now.
  - Rejected because that broadens scope into public/shared contracts instead of stabilizing the daemon seam first.
- Split aggressively by hypothetical future features (`devnet`, `fetchd`, `virtionetd`).
  - Rejected because it bakes in future assumptions before the current daemon seams are stabilized.

## Open questions

- Which extracted pieces should later move into shared, no_std-capable crates after the daemon seam is stabilized?
- Which bring-up-only debug markers should remain as stable observability helpers versus later cleanup candidates?

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

- [x] **Phase 0**: module boundary defined and reflected in `netstackd` structure — proof: `just diag-os`
- [x] **Phase 1**: behavior-preserving extraction completed for bootstrap/IPC/loopback/facade seams — proof: `RUN_UNTIL_MARKER=1 RUN_TIMEOUT=90s ./scripts/qemu-test.sh`
- [x] **Phase 2**: loop/idiom hardening completed with narrow host tests and unchanged marker/wire semantics — proof: `cargo test -p netstackd --tests -- --nocapture`
- [x] Task(s) linked with stop conditions + proof commands.
- [x] QEMU markers (if any) appear in `scripts/qemu-test.sh` and pass.
- [x] Security-relevant negative tests exist (`test_reject_*`).
