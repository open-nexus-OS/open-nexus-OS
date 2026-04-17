# RFC-0038: Selftest-client production-grade deterministic test architecture refactor v1

- Status: Draft
- Owners: @runtime
- Created: 2026-04-16
- Last Updated: 2026-04-17
- Links:
  - Execution SSOT: `tasks/TASK-0023B-selftest-client-production-grade-deterministic-test-architecture-refactor.md`
  - Follow-on task: `tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md`
  - ADRs:
    - `docs/adr/0005-dsoftbus-architecture.md`
  - Related RFCs:
    - `docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md`
    - `docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md`
    - `docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md`

## Status at a Glance

- **Phase 0 (contract seed + target architecture)**: ✅
- **Phase 1 (structural refactor without behavior change)**: 🟡 in-flight — Cuts 0–18 merged: `os_lite/{ipc, dsoftbus, net, mmio, vfs, timed, probes, services/{samgrd,bundlemgrd,keystored,policyd,execd,logd,metricsd,statefs,bootctl}}` extracted; `main.rs` at 122 lines; `os_lite/mod.rs` at ≈ 2025 lines (down from ~6771). Remaining: `updated` family + IPC-kernel/security probes + ELF helpers + `emit_line` shim.
- **Phase 2 (maintainability/extensibility optimization)**: ⬜ — opens after Phase 1 closure; first item is `pub fn run()` slicing into sub-orchestrators.
- **Phase 3 (production-grade closure + standards review)**: ⬜

Definition:

- “Complete” means the contract is defined and the proof gates are green (tests/markers). It does not mean “never changes again”.
- This RFC is the architecture/contract seed; `TASK-0023B` is the execution truth for stop conditions and proof commands.

## Scope boundaries (anti-drift)

This RFC is a design seed / contract. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - the architectural contract for refactoring `source/apps/selftest-client/src/main.rs` and its surrounding module boundaries,
  - the rule that `selftest-client` is a production-grade deterministic proof surface rather than incidental test glue,
  - the requirement that `main.rs` becomes minimal (entry + dispatch + high-level orchestration only),
  - the rule that the deterministic QEMU marker ladder and service-proof semantics remain behavior-equivalent through the refactor,
  - the rule that the initial target structure is adaptive and may be improved during refactor if the result is more maintainable and auditable.
- **This RFC does NOT own**:
  - new transport functionality, recovery features, or QUIC protocol expansion (owned by `TASK-0024` and later follow-ons),
  - kernel contract changes,
  - replacing the authoritative QEMU proof model with host-only coverage,
  - turning this refactor RFC into a backlog of future selftest features.

### Relationship to tasks (single execution truth)

- Tasks (`tasks/TASK-*.md`) define stop conditions and proof commands.
- This RFC defines the contract for the selftest-client refactor; `TASK-0023B` remains the execution single source of truth.
- This RFC must not become the place where phase completion, queue progress, or proof execution status is tracked beyond contract-level checklist context.

## Context

`source/apps/selftest-client/src/main.rs` currently acts as one of the most central proof surfaces in the OS:

- it orchestrates the deterministic QEMU marker ladder,
- it sequences routing, policy, VFS, OTA, networking, DSoftBus, exec, state, and observability probes,
- it acts as a release-truth path for service bring-up and proof closure.

That concentration makes future work expensive and risky:

- architectural review is hard because responsibilities are co-located,
- feature work reopens a monolith rather than a seam,
- deterministic proof behavior is harder to audit because orchestration and protocol logic are mixed,
- the current shape encourages re-monolithization over time.

Before `TASK-0024` extends transport behavior, this deterministic proof infrastructure must be hardened into a production-grade architecture.

## Goals

- Define a stable refactor contract for `selftest-client` as deterministic proof infrastructure.
- Reduce `main.rs` to a minimal entry/orchestration shell.
- Split `selftest-client` around explicit responsibilities (IPC, services, net, DSoftBus, markers, orchestration).
- Preserve full marker/proof semantics while making future change safer and more reviewable.

## Non-Goals

- Adding new QUIC/recovery/data-plane features.
- Changing existing marker names or proof semantics in this slice.
- Replacing task-owned proof commands with RFC-owned execution tracking.
- Freezing a rigid final folder layout before implementation reveals the best seams.

## Constraints / invariants (hard requirements)

- **Determinism**: marker ordering, bounded retry behavior, and proof semantics must remain deterministic.
- **No fake success**: no `ok`/`ready` marker semantics may change as part of the refactor.
- **Discovered dishonesty is in scope**:
  - if refactor work reveals logic bugs or fake-success markers, they must be fixed rather than preserved,
  - fake-success markers must be converted into real behavior/proof markers tied to verified outcomes.
- **Behavior-marker definition**:
  - a success marker is honest only after a verified state transition, assertion, or externally checked result,
  - entering a path, returning from a helper, or merely "not seeing an error yet" is not sufficient proof.
- **Bounded resources**: helper extraction must preserve bounded loops, parsers, and buffers.
- **Production-grade maintainability**:
  - boundaries must become easier to audit,
  - follow-on feature work must no longer require reopening a monolith,
  - the structure must resist re-monolithization.
- **Security floor**:
  - reject behavior for malformed/oversized inputs remains fail-closed,
  - no silent fallback marker drift is allowed,
  - no secret/session material is exposed through logs or marker churn.
- **Adaptive structure rule**:
  - the initial target structure is a starting model, not a fixed promise,
  - during implementation, module boundaries may change if the result is more maintainable, more explicit, and safer for deterministic proof evolution.

## Proposed design

### Contract / interface (normative)

This RFC defines the internal architecture contract for `selftest-client` v1 refactor:

- `main.rs` becomes a minimal shell:
  - cfg entry,
  - top-level dispatch,
  - high-level orchestration only.
- `main.rs` must not remain the home for:
  - protocol encode/decode logic,
  - service-specific RPC logic,
  - retry/state-machine loops,
  - marker business logic for subsystem probes.
- Review criteria for `main.rs` minimality:
  - no new subsystem-specific helper should accumulate there,
  - no parser/encoder/decoder should live there,
  - no reply-correlation, retry-budget, or deadline logic should live there,
  - no service-specific marker branching should live there.
- OS selftest logic moves behind explicit internal module seams under `src/os_lite/**`.
- The refactor is behavior-preserving:
  - same proof ordering,
  - same marker meanings,
  - same QEMU harness expectations,
  - same external service/protocol semantics.
- The architecture should separate at least these responsibility families:
  - deterministic orchestration,
  - marker helpers,
  - IPC/routing/client caches,
  - service-specific probes,
  - networking helpers,
  - DSoftBus local/remote probes,
  - MMIO/VFS and other peripheral proof helpers.

Initial target structure:

```text
source/apps/selftest-client/src/
  main.rs
  markers.rs
  os_lite/
    mod.rs
    ipc/
    services/
    net/
    mmio/
    dsoftbus/
    vfs.rs
```

Normative rule for the structure:

- this initial structure is authoritative only as a starting contract,
- if refactor work reveals better module boundaries, the structure should be updated rather than followed rigidly,
- such updates are valid only when they improve maintainability, ownership clarity, and deterministic proof auditability.
- extraction should, where sensible, follow the existing proof/harness phases so regressions stay local and reviewable.

### Phases / milestones (contract-level)

- **Phase 0**: contract seed exists; target architecture and invariants are explicit.
- **Phase 1**: structural extraction begins and `main.rs` starts shrinking without behavior change.
  - preferred first cuts:
    - DSoftBus QUIC/local transport leaf,
    - shared netstack/UDP helper seams,
    - IPC/routing/client-cache seams.
  - operational rule:
    - create module skeletons first,
    - move one responsibility slice at a time,
    - rerun parity proof after each major extraction cut,
    - stop immediately on marker/order/reject-path drift.
- **Phase 2**: broader module boundaries become maintainable/extensible instead of merely smaller.
  - optimize seams to match runtime/proof phases where practical so debugging and review stay local.
- **Phase 3**: production-grade closure is demonstrated:
  - `main.rs` is minimal,
  - deterministic proof paths are easy to audit,
  - Rust standards are reviewed and applied where sensible,
  - future work can extend the crate without re-centralizing it,
  - newly discovered logic bugs or fake-success markers have been converted into honest behavior/proof signals.

## Security considerations

- **Threat model**:
  - refactor drift changes proof semantics while claiming “no behavior change”,
  - extracted helper code weakens reject handling,
  - local structural cleanup harms the global deterministic proof model,
  - future contributors re-accumulate orchestration and protocol logic in `main.rs`.
- **Mitigations**:
  - keep the full proof floor green after each phase,
  - preserve the authoritative marker ladder semantics,
  - keep `main.rs` minimal by contract, not taste,
  - review `newtype`/ownership/`Send`/`Sync`/`#[must_use]` surfaces before closure,
  - convert any discovered dishonest marker path into a real behavior/proof marker rather than carrying it forward.
- **Open risks**:
  - the best final module seams may differ from the initial target structure,
  - some probe families may prove more tightly coupled than expected and require an adjusted intermediate layout.

## Failure model (normative)

- If marker names, marker ordering semantics, or QEMU proof expectations drift unintentionally, this RFC fails.
- If `main.rs` remains the effective storage location for most service/protocol logic after the refactor, this RFC fails.
- If the structure becomes smaller but not more maintainable/auditable, this RFC fails.
- If known logic bugs or fake-success markers are intentionally preserved unchanged, this RFC fails.
- If a new structural discovery implies a bigger architecture boundary, work must either:
  - adapt the target structure inside this RFC’s scope, or
  - stop and create a new RFC/ADR for the newly discovered boundary.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture
cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os
```

### Proof (Closure / hygiene)

```bash
cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os
```

### Deterministic markers (critical subset)

- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

Note:

- This RFC preserves the whole service-test ladder; the list above is only a critical subset, not the complete ladder contract.
- The complete ladder enforced by `scripts/qemu-test.sh` remains authoritative for closure.
- These marker names are not by themselves sufficient proof unless they remain tied to verified behavior.

## Alternatives considered

- Keep `TASK-0023B` narrow and only extract `quic_os/`.
  - Rejected because the real structural problem is broader than the QUIC leaf.
- Split the refactor into many tiny tasks immediately.
  - Rejected because the architecture cut would become artificial and lose the central “production-grade proof infrastructure” framing.
- Freeze a rigid target structure before implementation.
  - Rejected because the best module boundaries will partly emerge during refactor work.

## Open questions

- Should host-specific `run()` logic later move into a separate `host/` subtree, or remain a small leaf near `main.rs`?
- Which extracted helper families are likely to become reusable across future deterministic proof clients?

## RFC Quality Guidelines (for authors)

When updating this RFC, ensure:

- the adaptive-structure rule remains explicit,
- `main.rs` minimalization remains a hard outcome, not a soft preference,
- production-grade deterministic proof infrastructure remains the quality bar,
- proof commands stay concrete and task-aligned,
- discovered dishonest markers are converted into honest behavior/proof markers rather than normalized.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 0**: contract seed defined; initial target structure + invariants are explicit — proof: task + RFC linked.
- [ ] **Phase 1**: structural refactor begins and shrinks `main.rs` without behavior change — proof: `cargo test -p dsoftbusd -- --nocapture && just test-dsoftbus-quic && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`.
  - [x] Cuts 0–9: periphery extraction (`ipc/{clients,routing,reply}`, `mmio`, `vfs`, `net/{icmp_ping,local_addr,smoltcp_probe}`, `dsoftbus/{quic_os,remote/*}`, `timed`, `probes/{rng,device_key}`).
  - [x] Cuts 10–18: service-family extraction (`services/{samgrd,bundlemgrd,keystored,policyd,execd,logd,metricsd,statefs,bootctl}/mod.rs` + shared `services::core_service_probe*`).
  - [ ] Cut 19+: `updated` family (`updated_*`, `init_health_ok`, `SYSTEM_TEST_NXS`).
  - [ ] Cut 20+: IPC-kernel/security probes (`qos_probe`, `ipc_payload_roundtrip`, `ipc_deadline_timeout_probe`, `nexus_ipc_kernel_loopback_probe`, `cap_move_reply_probe`, `sender_pid_probe`, `sender_service_id_probe`, `ipc_soak_probe`).
  - [ ] Cut 21+: ELF helpers (`log_hello_elf_header`, `read_u64_le`) + `emit_line` shim consolidation.
  - [ ] Phase-1 closure: `wc -l main.rs` = 122 unchanged; `os_lite/mod.rs` reduced to imports + `run()` body + remaining glue; full Proof-Floor green.
- [ ] **Phase 2**: broader module boundaries optimized for maintainability/extensibility — proof: same phase proof floor. First Phase-2 deliverable: `pub fn run()` slicing into sub-orchestrators (`bring_up`, `mmio`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`).
- [ ] **Phase 3**: production-grade closure review complete (`main.rs` minimal, standards reviewed, proof paths auditable) — proof: same phase proof floor + `just dep-gate && just diag-os`.
- [x] Task linked with stop conditions + proof commands.
- [x] QEMU markers remain green in `scripts/qemu-test.sh` (verified after every cut so far).
- [x] Security-relevant negative behavior remains fail-closed (Cuts 0–18: reject paths preserved; `keystored_sign_denied`, `policyd_requester_spoof_denied`, `metricsd_security_reject_probe`, `statefs_unauthorized_access`, `logd_hardening_reject_probe` all intact).
- [ ] Any discovered logic-error or fake-success-marker path is converted into honest behavior/proof signaling — none discovered in Cuts 10–18; rule remains active for the remaining cuts.
