# RFC Process

1. Title your RFC `RFC-XXXX-short-title.md` with an incrementing number.
2. Describe the problem statement and constraints up front.
3. Outline the proposed design, alternatives, and validation strategy.
4. Document risks, mitigations, and test coverage expectations.
5. Submit a pull request and request subsystem maintainer reviews.

## Authority model (prevent drift)

We keep three document types with clear roles:

- **Tasks (`tasks/TASK-*.md`) are the execution truth**
  - They define **concrete work**, **stop conditions**, and **proof** (QEMU markers and/or `cargo test`).
  - They are updated as reality changes (new blockers, corrected scope, revised proof signals).
  - They must remain honest: no “fake success” markers; determinism rules apply.

- **RFCs (`docs/rfcs/RFC-*.md`) are design seeds / contracts**
  - They define **architecture decisions**, **interfaces/contracts**, and **what “stable” means** (if applicable).
  - They must not grow into a backlog tracker; link to tasks for implementation and evidence.
  - **Scope rule (keep RFCs “100% done”)**:
    - Each RFC should be scoped so it can realistically reach **Status: Complete** as soon as its
      corresponding task slice(s) are done and proven.
    - If a follow-on task needs new behavior beyond the existing RFC scope, create a **new RFC**
      (a new “contract seed”) instead of extending an old RFC into a multi-phase backlog.
    - When we intentionally defer a capability (e.g. “real subnet discovery”), the current RFC must
      state that it is **out of scope** and that a **new RFC** will define the next contract when scheduled.
  - If a contract changes, update the RFC *and* link to the task/PR that proves it.

- **ADRs (`docs/adr/*.md`) are narrow decision records**
  - Use ADRs for “one decision, one rationale” when a change is too granular or too cross-cutting to
    live inside a single RFC without causing churn.

### Contradictions rule

- If a task and an RFC disagree on **architecture/contract**, treat the **RFC as authoritative** and update the task.
- If they disagree on **progress/plan/proof signals**, treat the **task as authoritative** and update the RFC only if the *contract* changed.

### “Contract seed” rule for follow-on tasks

- Follow-on tasks MUST NOT silently expand old RFC scopes.
- If a follow-on task requires new contracts, add a new RFC (or ADR if it’s a narrow decision),
  link it from the new task, and keep the previous RFC marked **Complete**.

## RFC template (required structure)

Use the template when creating new RFCs:

- `docs/rfcs/RFC-TEMPLATE.md`

Hard requirements (agents should keep these current):

- **Status at a Glance** section near the top (phase-level progress).
- **Implementation Checklist** section at the end — **tracks implementation progress only**, not document quality.
  - Each phase should have a checkbox with proof command.
  - Checklists must reflect actual repo state (tests pass / markers appear).
  - Do NOT use checklists to track "is the RFC well-written" — that's review, not tracking.

## Security-relevant RFCs

RFCs touching crypto, auth, identity, capabilities, or sensitive data MUST include:

1. **Threat model**: What attacks are relevant?
2. **Security invariants**: What MUST always hold?
3. **DON'T DO list**: Explicit prohibitions
4. **Proof strategy**: How security is verified (negative tests, hardening markers)

See `docs/standards/SECURITY_STANDARDS.md` for detailed guidelines.

**Security RFCs in this repo:**

- RFC-0005: Kernel IPC & Capability Model (capability-based security)
- RFC-0008: DSoftBus Noise XK v1 (authentication + identity binding)
- RFC-0009: no_std Dependency Hygiene v1 (build security)
- RFC-0015: Policy Authority & Audit Baseline v1 (policy engine + audit trail)
- RFC-0017: Device MMIO Access Model v1 (capability-gated device access)
- RFC-0022: Kernel SMP v1b scheduler/SMP hardening contract (bounded queues + trap/IPI + CPU-ID path)
- RFC-0023: QoS ABI + timed coalescing contract v1 (authorization + bounded timer policy)
- RFC-0024: Observability v2 local contract (bounded metrics/tracing export via logd)
- RFC-0025: IPC liveness hardening v1 (bounded retry/correlation + deterministic timeout contract)
- RFC-0026: IPC performance optimization v1 (deterministic control-plane reuse + zero-copy-aligned data paths)
- RFC-0030: DSoftBus remote statefs RW v1 (authenticated RW + ACL + audit contract)
- RFC-0031: Crashdumps v1 (bounded artifacts + fail-closed crash metadata publish contract)
- RFC-0032: ABI syscall guardrails v2 (userland guardrail + authenticated profile distribution boundary)
- RFC-0033: DSoftBus streams v2 mux/flow-control/keepalive (authenticated mux + bounded credits/state transitions)
- RFC-0034: DSoftBus production closure v1 (legacy `TASK-0001..0020` production gates + hardening mapping)
- RFC-0035: DSoftBus QUIC v1 host-first scaffold contract (transport selection + fail-closed downgrade semantics + deterministic fallback markers)
- RFC-0036: DSoftBus core no_std transport abstraction v1 (no_std core boundary + transport abstraction + zero-copy-first and Rust safety discipline)
- RFC-0037: DSoftBus QUIC v2 OS enablement contract (real OS session markers + fail-closed reject/bounds evidence)
- RFC-0038: Selftest-client production-grade deterministic test architecture refactor v1 (deterministic proof infrastructure contract seed + minimal main requirement)
- RFC-0039: Supply-Chain v1 — bundle SBOM (CycloneDX) + repro metadata + signature allowlist policy (single-authority allowlist + deny-by-default install-time enforcement + deterministic deny markers)
- RFC-0045: Policy as Code v1 (single-authority unified policy tree + authenticated lifecycle + bounded explain/learn + `nx policy`) (Done)
- RFC-0046: UI v1a host CPU renderer + deterministic snapshots (Done; bounded renderer inputs + golden update gating + fake-marker prohibition)
- RFC-0047: UI v1b windowd surface/layer/present contract seed (Done; TASK-0055 Done)
- RFC-0048: UI v1c visible QEMU scanout bootstrap contract seed (Done; TASK-0055B Done)
- RFC-0049: UI v1d windowd visible present + SystemUI first-frame contract seed (Done; TASK-0055C execution SSOT)
- RFC-0050: UI v2a present scheduler + double-buffer + input routing contract (Done; TASK-0056 execution closure complete, next follow-up is `TASK-0056B`)
- RFC-0051: UI v2a visible input (cursor + focus + click) contract seed (In Progress; TASK-0056B execution SSOT)
- RFC-0040: Zero-Copy VMOs v1 plumbing contract seed (typed handle ownership contract + capability transfer discipline + host-first and OS-gated deterministic proof baseline)
- RFC-0041: PackageFS v2 read-only image + precomputed index fastpath contract seed (bounded mount validation + deterministic reject paths + host-first/OS-gated proofs)
- RFC-0042: Sandboxing v1 userspace confinement contract seed (namespace confinement + CapFd authenticity/replay reject + manifest permission bootstrap)

## Index

- RFC-0001: Kernel Simplification (Logic-Preserving)
  - docs/rfcs/RFC-0001-kernel-simplification.md
- RFC-0002: Process-Per-Service Architecture
  - docs/rfcs/RFC-0002-process-per-service-architecture.md
- RFC-0003: Unified Logging Infrastructure
  - docs/rfcs/RFC-0003-unified-logging.md
- RFC-0004: Loader Safety & Shared-Page Guards
  - docs/rfcs/RFC-0004-safe-loader-guards.md
- RFC-0005: Kernel IPC & Capability Model
  - docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
- RFC-0006: Userspace Networking v1 (virtio-net + smoltcp + sockets facade)
  - docs/rfcs/RFC-0006-userspace-networking-v1.md
- RFC-0007: DSoftBus OS Transport v1 (UDP discovery + TCP sessions over sockets facade)
  - docs/rfcs/RFC-0007-dsoftbus-os-transport-v1.md
- RFC-0008: DSoftBus Noise XK v1 (no_std handshake + identity binding)
  - docs/rfcs/RFC-0008-dsoftbus-noise-xk-v1.md
- RFC-0009: no_std Dependency Hygiene v1
  - docs/rfcs/RFC-0009-no-std-dependency-hygiene-v1.md
- RFC-0010: DSoftBus Cross-VM Harness v1
  - docs/rfcs/RFC-0010-dsoftbus-cross-vm-harness-v1.md
- RFC-0011: logd journal + crash reports v1
  - docs/rfcs/RFC-0011-logd-journal-crash-v1.md
- RFC-0012: Updates & Packaging v1.0 — System-Set (.nxs) + userspace-only A/B skeleton (non-persistent)
  - docs/rfcs/RFC-0012-updates-packaging-ab-skeleton-v1.md
- RFC-0013: Boot gates v1 — readiness contract + spawn failure reasons + resource/leak sentinel
  - docs/rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md
- RFC-0014: Testing contracts v1 — host-first service contract tests + phased QEMU smoke gates
  - docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md
- RFC-0015: Policy Authority & Audit Baseline v1 — policy engine + audit trail + policy-gated ops
  - docs/rfcs/RFC-0015-policy-authority-audit-baseline-v1.md
- RFC-0016: Device Identity Keys v1 — virtio-rng + rngd authority + keystored keygen
  - docs/rfcs/RFC-0016-device-identity-keys-v1.md
- RFC-0017: Device MMIO Access Model v1 — capability-gated MMIO + init-controlled distribution
  - docs/rfcs/RFC-0017-device-mmio-access-model-v1.md
- RFC-0018: StateFS Journal Format v1 — journaled KV store for /state persistence
  - docs/rfcs/RFC-0018-statefs-journal-format-v1.md
- RFC-0019: IPC Request/Reply Correlation v1 — nonce correlation + shared inbox determinism
  - docs/rfcs/RFC-0019-ipc-request-reply-correlation-v1.md
- RFC-0020: Kernel ownership + Rust idioms pre-SMP v1 (logic-preserving)
  - docs/rfcs/RFC-0020-kernel-ownership-and-rust-idioms-pre-smp-v1.md
- RFC-0021: Kernel SMP v1 contract — per-CPU runqueues + IPI resched (Complete)
  - docs/rfcs/RFC-0021-kernel-smp-v1-percpu-runqueues-ipi-contract.md
- RFC-0022: Kernel SMP v1b hardening contract — bounded scheduler queues + trap/IPI hardening + CPU-ID fast-path/fallback (Complete)
  - docs/rfcs/RFC-0022-kernel-smp-v1b-scheduler-hardening-contract.md
- RFC-0023: QoS ABI + timed coalescing contract v1 (Complete)
  - docs/rfcs/RFC-0023-qos-abi-timed-coalescing-contract-v1.md
- RFC-0024: Observability v2 local contract - metricsd + tracing export via logd (Complete)
  - docs/rfcs/RFC-0024-observability-v2-metrics-tracing-contract-v1.md
- RFC-0025: IPC liveness hardening v1 - bounded retry/correlation + deterministic timeout contract (Complete)
  - docs/rfcs/RFC-0025-ipc-liveness-hardening-bounded-retry-contract-v1.md
- RFC-0026: IPC performance optimization v1 - deterministic control-plane reuse + zero-copy-aligned data paths (Complete)
  - docs/rfcs/RFC-0026-ipc-performance-optimization-contract-v1.md
- RFC-0027: DSoftBusd modular daemon structure v1 (Complete)
  - docs/rfcs/RFC-0027-dsoftbusd-modular-daemon-structure-v1.md
- RFC-0028: DSoftBus remote packagefs RO v1 (Complete)
  - docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md
- RFC-0029: Netstackd modular daemon structure v1 (Complete)
  - docs/rfcs/RFC-0029-netstackd-modular-daemon-structure-v1.md
- RFC-0030: DSoftBus remote statefs RW v1 (Complete)
  - docs/rfcs/RFC-0030-dsoftbus-remote-statefs-rw-v1.md
- RFC-0031: Crashdumps v1 - deterministic in-process minidumps + host symbolization (Complete)
  - docs/rfcs/RFC-0031-crashdumps-v1-minidump-host-symbolize.md
- RFC-0032: ABI syscall guardrails v2 - userland guardrail (Complete; policyd-only profile authority with deterministic proof closure)
  - docs/rfcs/RFC-0032-abi-syscall-guardrails-v2-userland-kernel-untouched.md
- RFC-0033: DSoftBus streams v2 mux/flow-control/keepalive - host-first contract seed (Done; TASK-0020 closure proofs green including single-VM/2-VM/perf/soak/release-evidence)
  - docs/rfcs/RFC-0033-dsoftbus-streams-v2-mux-flow-control-keepalive.md
- RFC-0034: DSoftBus production closure v1 - legacy `TASK-0001..0020` closure contract (Done; obligations extracted/proven under TASK-0020, no >0020 scope)
  - docs/rfcs/RFC-0034-dsoftbus-production-closure-v1.md
- RFC-0035: DSoftBus QUIC v1 host-first scaffold contract (Done; `TASK-0021` closure proofs synced with explicit `TASK-0022` boundary)
  - docs/rfcs/RFC-0035-dsoftbus-quic-v1-host-first-os-scaffold.md
- RFC-0036: DSoftBus core no_std transport abstraction v1 (Complete; `TASK-0022` is Done)
  - docs/rfcs/RFC-0036-dsoftbus-core-no-std-transport-abstraction-v1.md
- RFC-0037: DSoftBus QUIC v2 OS enablement gated contract (Complete; `TASK-0023` now proves real OS QUIC session path and QUIC-required marker contract)
  - docs/rfcs/RFC-0037-dsoftbus-quic-v2-os-enabled-gated.md
- RFC-0038: Selftest-client production-grade deterministic test architecture refactor v1 (Done 2026-04-20; `TASK-0023B` is `In Review` with all six phases functionally closed — proof-manifest SSOT + schema-v2 split, signed evidence bundles, replay/diff/bisect tooling with bounded budgets, cross-host determinism allowlist; one environmental closure step remaining for P6-05: external CI-runner replay artifact, see `docs/testing/replay-and-bisect.md` §7-§11)
  - docs/rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md
- RFC-0039: Supply-Chain v1 — bundle SBOM (CycloneDX JSON per ADR-0021) + repro metadata + single-authority publisher/key allowlist (Done; proof checklist complete and green; execution task `TASK-0029` is `Done`; v2/v3 boundaries unchanged: `TASK-0197`/`TASK-0198`/`TASK-0289`)
  - docs/rfcs/RFC-0039-supply-chain-v1-bundle-sbom-repro-sign-policy.md
- RFC-0040: Zero-Copy VMOs v1 plumbing — host-first, OS-gated contract seed (Done; Phase 0/1 proofs green, kernel production closure explicitly delegated to `TASK-0290`)
  - docs/rfcs/RFC-0040-zero-copy-vmos-v1-plumbing-host-first-os-gated.md
- RFC-0041: PackageFS v2 read-only image + precomputed index fastpath — host-first, OS-gated contract seed (Done; execution SSOT `TASK-0032` is Done with host+OS proof gates)
  - docs/rfcs/RFC-0041-packagefs-v2-ro-image-index-fastpath-host-first-os-gated.md
- RFC-0042: Sandboxing v1 userspace confinement (VFS namespaces + CapFd + manifest permissions) — host-first, OS-gated contract seed (Done; execution SSOT `TASK-0039` is Done)
  - docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md
- RFC-0043: DevX nx CLI v1 (host-first, production-floor) — single entrypoint contract seed (Done; execution SSOT `TASK-0045` is Done)
  - docs/rfcs/RFC-0043-devx-nx-cli-v1-host-first-production-floor-seed.md
- RFC-0044: Config v1 (`configd` + schemas + layering + 2PC + `nx config`) — host-first, OS-gated contract seed (Done; host proof floor green, OS marker closure delegated to downstream follow-ups; execution SSOT `TASK-0046` is Done)
  - docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md
- RFC-0045: Policy as Code v1 (unified policy tree + evaluator + explain/dry-run + learn→enforce + `nx policy`) — host-first, OS-gated contract seed (Done; host proof floor green, OS marker closure intentionally unclaimed)
  - docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md
- RFC-0046: UI v1a host CPU renderer + deterministic snapshots — host-first contract seed (Done; TASK-0054 Done with host proofs green and no OS/QEMU marker claims)
  - docs/rfcs/RFC-0046-ui-v1a-host-cpu-renderer-snapshots-contract.md
- RFC-0047: UI v1b windowd surface/layer/present contract seed (Done; TASK-0055 Done)
  - docs/rfcs/RFC-0047-ui-v1b-windowd-surface-layer-present-contract.md
- RFC-0048: UI v1c visible QEMU scanout bootstrap contract seed (Done; TASK-0055B Done)
  - docs/rfcs/RFC-0048-ui-v1c-visible-qemu-scanout-bootstrap-contract.md
- RFC-0049: UI v1d windowd visible present + SystemUI first-frame contract seed (Done; TASK-0055C execution SSOT)
  - docs/rfcs/RFC-0049-ui-v1d-windowd-visible-present-systemui-first-frame-contract.md
- RFC-0050: UI v2a present scheduler + double-buffer + input routing contract (Done; TASK-0056 execution closure complete, next follow-up is `TASK-0056B`)
  - docs/rfcs/RFC-0050-ui-v2a-present-scheduler-double-buffer-input-routing-contract.md
- RFC-0051: UI v2a visible input (cursor + focus + click) contract seed (In Progress; TASK-0056B execution SSOT)
  - docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md
