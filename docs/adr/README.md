# Architecture Decision Records (ADRs)

ADRs are narrow decision records: one decision, one rationale, and its consequences — used when a
change is too granular or too cross-cutting to live inside a single RFC without causing churn.
RFCs (design contract seeds: interfaces, invariants, proof strategy) live in [`../rfcs/`](../rfcs/);
tasks (`tasks/TASK-*.md`) remain the execution truth. To add an ADR: copy [`template.md`](template.md),
take the next free number, fill it in, and add an index line below.

Note: **ADR-0019 was never filed; the number is retired to keep history stable.**

## Index

- [ADR-0001: Runtime Roles & Boundaries (Host + OS-lite)](0001-runtime-roles-and-boundaries.md) — Accepted
- [ADR-0002: Nexus Loader Architecture](0002-nexus-loader-architecture.md) — Accepted
- [ADR-0003: IPC Runtime Architecture](0003-ipc-runtime-architecture.md) — Accepted
- [ADR-0004: IDL Runtime Architecture](0004-idl-runtime-architecture.md) — Accepted
- [ADR-0005: DSoftBus-lite Architecture](0005-dsoftbus-architecture.md) — Accepted
- [ADR-0006: Device Identity Architecture](0006-device-identity-architecture.md) — Accepted
- [ADR-0007: Executable Payloads Architecture](0007-executable-payloads-architecture.md) — Accepted
- [ADR-0008: Clipboard Architecture](0008-clipboard-architecture.md) — Accepted
- [ADR-0009: Bundle Manager Architecture](0009-bundle-manager-architecture.md) — Accepted (updated 2026-01-22 for manifest.nxb unification)
- [ADR-0010: Search Architecture](0010-search-architecture.md) — Accepted
- [ADR-0011: Settings Architecture](0011-settings-architecture.md) — Accepted
- [ADR-0012: Time Sync Architecture](0012-time-sync-architecture.md) — Accepted
- [ADR-0013: Notification Architecture](0013-notification-architecture.md) — Accepted
- [ADR-0014: Policy Architecture](0014-policy-architecture.md) — Accepted (original recipe-directory baseline superseded by Policy as Code v1 — RFC-0045 / TASK-0047)
- [ADR-0015: Resource Manager Architecture](0015-resource-manager-architecture.md) — Accepted
- [ADR-0016: Kernel Libraries Architecture](0016-kernel-libs-architecture.md) — Accepted
- [ADR-0017: Service Architecture](0017-service-architecture.md) — Accepted
- [ADR-0018: DriverKit ABI Versioning and Stability](0018-driverkit-abi-versioning-and-stability.md) — Proposed
- ADR-0019 — never filed; number retired
- [ADR-0020: Bundle Manifest Format (manifest.nxb with Cap'n Proto)](0020-manifest-format-capnproto.md) — Accepted
- [ADR-0021: Structured Data Formats (JSON vs Cap'n Proto)](0021-structured-data-formats-json-vs-capnp.md) — Accepted
- [ADR-0022: Modern image formats (WebP/AVIF) for wallpapers, screenshots, and thumbnail caches](0022-modern-image-formats-avif-webp.md) — Proposed
- [ADR-0023: StateFS Persistence Architecture](0023-statefs-persistence-architecture.md) — Accepted
- [ADR-0024: Updates A/B Packaging Architecture](0024-updates-ab-packaging-architecture.md) — Accepted
- [ADR-0025: QEMU Smoke Proof Gating (Networking + DSoftBus)](0025-qemu-smoke-proof-gating.md) — Accepted
- [ADR-0026: Network Address Profiles and Validation Semantics](0026-network-address-profiles-and-validation.md) — Accepted
- [ADR-0027: selftest-client Two-Axis Architecture](0027-selftest-client-two-axis-architecture.md) — Accepted
- [ADR-0028: windowd surface/present and visible bootstrap architecture](0028-windowd-surface-present-and-visible-bootstrap-architecture.md) — Accepted (amended 2026-06-02: GPU-only architecture per RFC-0059 Phase 6)
- [ADR-0029: input v1 host core architecture](0029-input-v1-host-core-architecture.md) — Accepted
- [ADR-0030: Layout Engine — Deterministic Pretext Philosophy](0030-layout-engine-deterministic-pretext.md) — Accepted
- [ADR-0031: Three-Layer Animation Architecture (Animation Engine + NexusGfx SDK + GPU Driver)](0031-three-layer-animation-architecture.md) — Accepted
- [ADR-0032: GPU Command Ring + Pipelined Present (gpud virtio-gpu)](0032-gpu-command-ring-and-pipelined-present.md) — Accepted
- [ADR-0033: Soft-real-time spine — waitset + timeline fence + DriverKit submit](0033-soft-real-time-spine.md) — Accepted (Phases 1–4; Phase 5 clean idle deferred)
- [ADR-0034: Reactive cursor architecture — hardware overlay first, decoupled software fallback](0034-reactive-cursor-architecture.md) — Accepted
- [ADR-0035: SystemUI declarative shell configuration — manifests resolve the shell, windowd renders it](0035-systemui-declarative-shell-configuration.md) — Accepted
- [ADR-0036: App lifecycle, process spawn, and app registry are three separate services — `abilitymgr`, `execd`, `bundlemgrd` (no new `appmgrd`)](0036-ability-lifecycle-vs-process-vs-registry-service-split.md) — Accepted
- [ADR-0037: Each app owns its surface VMO, lazily allocated when active and freed when closed](0037-per-app-surface-lazy-vmo-lifecycle.md) — Accepted
- [ADR-0038: One Rust SSOT for the windowd↔gpud display wire; Cap'n Proto stays the control plane](0038-display-wire-ssot-and-capnp-boundary.md) — Accepted
- [ADR-0039: Device-class driver architecture — bus-HAL + DriverKit + a thin device shim](0039-device-class-driver-architecture.md) — Accepted
- [ADR-0040: Unified logging policy, timing signposts, and the pure-observer proof model](0040-unified-logging-policy-and-observer-proof-model.md) — Accepted
- [ADR-0041: Never-black boot — held GPU splash → atomic desktop reveal](0041-never-black-boot-splash-atomic-desktop-reveal.md) — Accepted
- [ADR-0042: Cross-process surface transport — per-app VMO + present IPC + compositor blit (v1)](0042-cross-process-surface-transport.md) — Accepted
- [ADR-0043: User data lives in a dedicated filesystem service (`nxfs`); statefs stays the service-state KV](0043-user-data-in-dedicated-cow-fs-statefs-stays-service-kv.md) — Accepted
- [ADR-0044: Block-device layout for statefs + nxfs — two virtio-blk devices; GPT consolidation follow-up](0044-single-blk-device-gpt-partitions-block-layer.md) — Accepted (amended 2026-07-15 after TASK-0293 bring-up)
- [ADR-0045: pinched — system-internal compute broker with exchangeable backends](0045-pinched-compute-broker-and-backends.md) — Accepted
- [ADR-0046: nexus-workpool — deterministic same-AS parallel compute](0046-deterministic-parallel-compute-workpool.md) — Accepted
- [ADR-0047: nexus-inet — minimal interaction-net evaluator backend](0047-interaction-net-evaluator-backend.md) — Accepted
- [ADR-0048: standing userspace runtime-integrity detectors](0048-userspace-runtime-integrity-detectors.md) — Accepted
- [ADR-0049: BKL lock-classes + soft-realtime CPU placement (SMP=4 interactive)](0049-bkl-lockclass-and-softrt-cpu-placement.md) — Accepted
