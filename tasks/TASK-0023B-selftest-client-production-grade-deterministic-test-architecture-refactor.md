---
title: TASK-0023B Selftest-Client production-grade deterministic test architecture refactor v1
status: Draft
owner: @runtime
created: 2026-04-16
depends-on:
  - TASK-0023
follow-up-tasks:
  - TASK-0024
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Contract seed: docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md
  - Depends-on (OS QUIC session baseline): tasks/TASK-0023-dsoftbus-quic-v2-os-enabled-gated.md
  - Follow-up (transport hardening): tasks/TASK-0024-dsoftbus-udp-sec-v1-os-enabled.md
  - Testing contract: scripts/qemu-test.sh
---

## Short description

- **Scope**: Refactor `source/apps/selftest-client/src/main.rs` and the surrounding selftest-client architecture into a scalable deterministic-test structure.
- **Deliver**: multi-phase no-behavior-change refactor, adaptive target structure, and a final state where `main.rs` is minimal and the selftest-client architecture reaches a production-grade quality bar.
- **Out of scope**: New transport features (owned by `TASK-0024`) and protocol semantic changes.

## Why this task exists

`source/apps/selftest-client/src/main.rs` is too large for safe iterative development.
More importantly, `selftest-client` is one of the central components of this OS because it drives the deterministic QEMU marker ladder and service-proof orchestration.
Before adding transport features in `TASK-0024`, we need a maintainable and extensible structure that keeps the full service-test ladder stable.

## Goal

After completion:

- `selftest-client` is organized around clear deterministic-test responsibilities rather than one monolithic file.
- `main.rs` is minimal: cfg entry + top-level dispatch/orchestration only.
- The full service-test structure and canonical QEMU marker ladder remain unchanged and green.
- The resulting deterministic test infrastructure is production-grade: maintainable, extensible, deterministic under pressure, and strict about proof integrity.
- Rust discipline review is done and documented where sensible (`newtype`, ownership, `Send`/`Sync`, `#[must_use]`).

## Target quality bar

This task targets **production-grade** quality for the deterministic proof infrastructure carried by `selftest-client`.

Reason:

- `selftest-client` is part of the release-truth path for QEMU/service closure claims.
- If this architecture is brittle, opaque, or hard to evolve safely, the whole deterministic proof story becomes weaker.
- This is therefore not just a cleanup task; it is hardening of a release-critical testing surface.

## Non-Goals

- No new QUIC data-plane/recovery features.
- No marker renaming or semantic drift in the existing deterministic service-test ladder.
- No mandatory creation of new unit tests solely for refactor cosmetics.
- No kernel changes.

## Constraints / invariants (hard requirements)

- Behavioral parity: same success/failure semantics as before refactor.
- Marker honesty: no new fake-success markers.
- Deterministic bounded loops and parsing paths remain intact.
- Keep ownership boundaries explicit; avoid large mutable shared state blobs.
- The deterministic test ladder is the product here, not incidental test glue.
- Production-grade maintainability is required: the resulting structure must make future changes safer, not just move code around.
- If refactor work reveals logic bugs, marker dishonesty, or fake-success markers, the task must fix them instead of preserving them.
- When fake-success markers are found, they must be replaced by real behavior markers/proofs tied to actual verified outcomes.

## Deterministic testing role (explicit)

`selftest-client` is not just a test binary.
It is the orchestrator for deterministic OS proof in QEMU and therefore a first-class architecture surface.
This task must preserve that role while improving maintainability and extensibility.

## Canonical proof contract (full ladder authority)

- The authoritative proof contract is the full QEMU ladder enforced by `scripts/qemu-test.sh`, not only the QUIC subset.
- Any refactor phase that keeps a small subset green but regresses the wider ladder is considered a failure.
- QUIC markers remain a critical subset, but this task protects the whole service-proof structure.

Behavior-marker rule:

- A marker counts as an honest behavior/proof marker only when it is emitted after a real verified condition or assertion.
- A marker does not count as honest proof when it only follows:
  - entering a code path,
  - returning from a helper call,
  - reaching an expected branch without validating the end condition,
  - assuming success because no error was observed yet.

## Initial target structure (explicitly adaptive, not rigid)

Initial target structure for this task:

```text
source/apps/selftest-client/src/
  main.rs
  markers.rs
  os_lite/
    mod.rs
    ipc/
      mod.rs
      clients.rs
      routing.rs
      reply.rs
      probes.rs
    services/
      keystored.rs
      samgrd.rs
      bundlemgrd.rs
      policyd.rs
      updated.rs
      execd.rs
      logd.rs
      statefs.rs
      bootctl.rs
      metrics.rs
    net/
      mod.rs
      netstack_rpc.rs
      local_addr.rs
      icmp_ping.rs
    mmio/
      mod.rs
    dsoftbus/
      mod.rs
      quic_os/
        mod.rs
        types.rs
        frame.rs
        udp_ipc.rs
        session_probe.rs
        markers.rs
      remote.rs
    vfs.rs
```

Notes:

- This structure is an initial target model, not a rigid final promise.
- If the refactor reveals better module boundaries, the structure should be adjusted to achieve the best maintainable result.
- Any structural adjustment during the refactor is allowed and desired when it improves:
  - deterministic test clarity,
  - ownership/module boundaries,
  - maintainability/extensibility,
  - reduction of protocol/business logic inside `main.rs`.
- `main.rs` should end as minimal as realistically possible: entrypoint + high-level orchestration only.

Normative end-state for `main.rs`:

- `main.rs` MAY contain:
  - cfg-gated entry wiring,
  - top-level dispatch into host/os-lite runners,
  - high-level phase/lifecycle orchestration.
- `main.rs` MUST NOT remain the home for:
  - service-specific RPC implementations,
  - protocol frame encode/decode logic,
  - retry loops or parser state machines,
  - marker-string business logic for subsystem probes.

Review gate for `main.rs` minimality:

- no new helper in `main.rs` should own subsystem-specific behavior,
- no parser/decoder/encoder should live in `main.rs`,
- no retry counters, reply-matching loops, or deadline machinery should live in `main.rs`,
- no service-specific marker text or proof-state branching should be introduced in `main.rs`.

## Touched paths (allowlist)

- `source/apps/selftest-client/src/main.rs`
- `source/apps/selftest-client/src/**` (new/refactored modules)
- `docs/testing/index.md` (only if proof command list changes)
- `tasks/STATUS-BOARD.md`
- `tasks/IMPLEMENTATION-ORDER.md`

## Execution phases (mandatory sequence)

Each phase must end with the phase proof floor before the next phase starts.

### Phase 1 - structural refactor only (no behavior change)

Scope:

- Create and wire the initial target structure (or a justified improved variant discovered during the work).
- Move logic out of `main.rs` without changing runtime behavior, marker ordering, or transport semantics.
- Extract first deterministic-test responsibility seams so `main.rs` immediately shrinks.
- Keep symbols/flows equivalent; this phase is decomposition only.

Preferred extraction order inside Phase 1:

1. DSoftBus local QUIC leaf (`os_lite/dsoftbus/quic_os/`)
2. shared netstack/UDP helper seams (`os_lite/net/`)
3. IPC/routing/client-cache seams (`os_lite/ipc/`)
4. service probe families and remaining peripheral helpers

Reason:

- extraction order should mirror the deterministic proof/harness structure as much as possible so failures stay local and reviewable.

Operational Phase-1 sequence:

1. create destination module skeletons and wire `mod.rs`/imports only,
2. move one responsibility slice at a time without semantic edits,
3. rerun the phase proof floor after each major extraction cut, not only at phase end,
4. stop and fix parity immediately if a moved slice changes marker behavior, ordering, or reject behavior.

Phase-1 proof floor:

- `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture`
- `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
- `cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`

### Phase 2 - maintainability/extensibility optimization

Scope:

- Improve boundaries and readability across the broader selftest-client structure, not just QUIC code.
- Reduce coupling between orchestration, service helpers, IPC/routing helpers, DSoftBus helpers, and marker policy.
- Introduce small typed wrappers where they reduce accidental misuse.
- Keep behavior unchanged; still no new transport features.
- Optimize toward a production-grade end state, not merely a smaller file.

Preferred optimization order:

1. align helper boundaries to harness/runtime phases (`bring-up`, `mmio`, `routing`, `ota`, `policy`, `logd`, `vfs`, `end`) where sensible,
2. isolate shared retry/deadline/reply-matching logic,
3. remove duplication that would otherwise re-couple probes into `main.rs`.

Phase-2 proof floor:

- Same commands as Phase 1 (must stay green).

### Phase 3 - closure review and standards check

Scope:

- Verify docs/status surfaces reflect the final refactor state.
- Review and apply Rust standards where sensible:
  - `newtype` wrappers for safety-relevant IDs/state selectors,
  - explicit ownership transfer boundaries,
  - `Send`/`Sync` assumptions reviewed (no unsafe shortcuts),
  - `#[must_use]` on decision-bearing results where useful.
- Verify that the final architecture leaves `main.rs` minimal and prevents re-monolithization.
- Verify that the final architecture is production-grade in practice:
  - responsibilities are legible,
  - critical deterministic proof paths are easy to audit,
  - follow-up feature work can land without re-centralizing the crate.

Mandatory anti-re-monolithization review:

- new logic added during the refactor must land in seam modules, not flow back into `main.rs`,
- newly discovered proof bugs or fake-success markers must be corrected into honest behavior markers/proofs,
- the resulting structure must be easier to extend for future tasks without collapsing orchestration and implementation back together.

Phase-3 proof floor:

- Same commands as Phase 1 (must stay green).
- `cd /home/jenning/open-nexus-OS && just dep-gate && just diag-os`

## Security considerations

### Threat model

- Refactor drift may accidentally bypass reject checks or marker gating.
- Parser/helper extraction may introduce subtle truncation/length bugs.

### Security invariants (MUST hold)

- Reject behavior for malformed/oversized frames remains fail-closed.
- Existing deterministic service-test marker contract remains unchanged.
- No silent fallback marker reintroduction in QUIC-required profile.
- Any discovered fake-success or logic-error marker path must be converted to honest behavior proof before closure.

### DON'T DO

- DON'T change protocol semantics under "refactor" label.
- DON'T ship refactor without parity proofs.
- DON'T hide new behavior behind renamed markers.
- DON'T optimize local structure while damaging the global deterministic test architecture.
- DON'T preserve dishonest markers just because they pre-date the refactor.

## Security proof

### Required tests / commands

- This is primarily a refactor task for selftest code; adding many new standalone tests is optional.
- Mandatory closure proof is parity/regression evidence after each phase.

- `cd /home/jenning/open-nexus-OS && cargo test -p dsoftbusd -- --nocapture`
- `cd /home/jenning/open-nexus-OS && just test-dsoftbus-quic`
- `cd /home/jenning/open-nexus-OS && REQUIRE_DSOFTBUS=1 RUN_UNTIL_MARKER=1 RUN_TIMEOUT=220s just test-os`

### Required QEMU markers (unchanged from TASK-0023)

- This task preserves the whole service-test ladder; the QUIC-required markers below are only a critical subset.
- The complete expected ladder in `scripts/qemu-test.sh` remains authoritative.
- `dsoftbusd: transport selected quic`
- `dsoftbusd: auth ok`
- `dsoftbusd: os session ok`
- `SELFTEST: quic session ok`

### Forbidden markers in QUIC-required profile

- `dsoftbusd: transport selected tcp`
- `dsoftbus: quic os disabled (fallback tcp)`
- `SELFTEST: quic fallback ok`

## Stop conditions (Definition of Done)

1. Phase 1-3 completed in order, with green proof floor after each phase.
2. The initial target structure exists or has been intentionally improved during the refactor with better module boundaries.
3. `main.rs` is minimal and no longer acts as monolithic storage for service/protocol logic.
4. No behavior regressions in host/service and QEMU proof floors.
5. The broader deterministic service-test structure remains green and unchanged, including the `TASK-0023` QUIC marker subset.
6. Rust standards closure review is complete and reflected in touched code/docs where sensible.
7. The resulting architecture meets a production-grade bar for deterministic proof infrastructure rather than a one-off refactor-only bar.
8. Any discovered logic bugs or fake-success markers have been converted into honest behavior/proof markers rather than preserved.
9. `TASK-0024` depends-on/queue metadata remains `TASK-0023B` first.

## Plan (small PRs)

1. **Phase 1 PR**: create scalable `os_lite` structure and start shrinking `main.rs` without behavior changes.
2. **Phase 2 PR**: improve broader module boundaries for maintainability/extensibility (still no behavior changes).
3. **Phase 3 PR**: standards/documentation closure review, ensure `main.rs` stays minimal, and rerun full proof floor.

## SSOT rule

- This task is the execution single source of truth for:
  - phase completion,
  - proof commands,
  - stop conditions,
  - queue/dependency updates.
- The RFC may define architecture intent and constraints, but must not become the execution tracker for this work.
