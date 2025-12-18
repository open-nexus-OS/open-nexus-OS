# Agent Vision (Open Nexus OS)

This document captures the **default product and architecture vision** that agents should use as
context when implementing changes, so users do not have to restate it in every session.

## North Star

Build a **universal, secure, and high-performance OS** that scales across:

- phone / tablet / desktop
- TV / automotive
- IoT and embedded
- networked/distributed deployments (future `softbusd`)

The OS should be **RISC‑V optimized** and aligned with the direction of **HarmonyOS**: an ecosystem
of devices that “feels like one system” to the user, with a distributed service layer.

The architecture must be **Rust-first** and **RISC‑V-first**: prefer Rust-idiomatic designs,
stable interfaces, and simple invariants that compile cleanly without endless feature-flag
workarounds.

It must feel **extremely fast** (low-latency interactions) while also supporting **long battery
life**. On desktop-class machines it must additionally support **professional peak performance**
when it is appropriate (e.g., rendering / graphics / heavy compute), without turning the default
experience into a power-hungry system.

## Principles (decision lens)

When choosing between designs, prefer solutions that:

- **Preserve security boundaries**: capability-based authority, least privilege, no ambient authority.
- **Aim for microkernel-hard security**: architecture-first security that is as close as practical to
  “military-grade” while remaining maintainable for a consumer OS (updates, developer velocity, and
  performance still matter).
- **Stay performance-scalable**: small fastpaths, avoid unnecessary copies; use VMO/filebuffer for bulk.
- **Support both efficiency and peak performance**: default to energy-efficient paths and enable
  explicit “burst/perf” modes for professional workloads.
- **Keep kernel minimal**: push complexity (IDL parsing, policy, crypto, distributed routing) to userland services.
- **Remain testable**: host-first tests + minimal authoritative QEMU E2E; no “success logs” without behavior.
- **Avoid lock-in**: designs should not hardcode “desktop only” or “IoT only”; keep a common core.
- **Do not copy seL4 (or any reference OS) 1:1**: take only what fits our constraints. If an idea
  forces Rust into unnatural contortions (endless cfg/feature flag matrices, unsafe glue, or “API
  gymnastics”), prefer a simpler Rust-idiomatic alternative.

## Architecture stance (current direction)

- **Control plane**: Cap’n Proto IDL in userland (typed messages).
- **Data plane**: VMO/filebuffer for large payloads (low/zero copy).
- **Local IPC**: kernel-enforced endpoint capabilities (RFC‑0005).
- **Policy**: `policyd` decides who gets what; kernel enforces rights on held caps.
- **Discovery/service graph**: `samgrd` registers/resolves services (OHOS-aligned).
- **Distributed (future)**: `softbusd` is layered in userland; kernel ABI stays local/portable.

## Hybrid security-root roadmap (default)

We deliberately use a hybrid approach:

- **MVP root**: verified boot + signed bundles/packages + policy gating + capability enforcement.
  This yields strong architecture-first security without blocking development on device-specific
  hardware.
- **Pluggable key custody**: design `keystored`/`identityd` so key material can migrate from a
  software backend (host/QEMU bring-up) to secure hardware (Secure Element / TEE / TPM-like) per
  device class without ABI churn.
- **Measured boot + attestation (later)**: add device attestation as an additive layer for the
  HarmonyOS-like device mesh (`softbusd`), keeping the kernel small and the trust decisions in
  userland policy.

## Distributed vision (softbusd, later)

The long-term direction is a HarmonyOS-like “device mesh”:

- `softbusd` provides discovery, secure sessions, and routing so services can appear “local” across
  devices.
- Kernel IPC remains local and capability-based; distributed behavior is expressed in userland and
  is policy-gated.

## What agents should do by default

- Interpret user decisions and requests **in relation to this vision**.
- If a request seems to conflict with the vision (security/perf/testability), call it out early.
- Propose **better implementations** when they are clearly aligned with the vision, with concrete tradeoffs.
- Keep stubs honest: label them, never fake success markers, and always provide proof of real behavior.
