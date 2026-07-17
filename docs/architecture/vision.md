# Agent Vision (Open Nexus OS)

This document captures the **default product and architecture vision** agents should use when implementing changes.

## North Star

Build a **secure, high-performance OS core** that scales across phone, desktop, TV, automotive, and embedded classes.

- **RISC-V-first, Rust-first**: favor Rust-idiomatic designs, stable interfaces, and simple invariants.
- **Fast by default**: low-latency interactions with energy-aware defaults.
- **Peak mode when explicit**: allow burst performance for pro workloads without making the baseline power-hungry.
- **Local sovereignty**: support local development and self-hosted operations where policy allows.
- **Managed posture support**: family, school, enterprise, and fleet policies share one coherent authority model.
- **Distributed fabric trajectory**: services should feel local across trusted devices via userland distributed layers.

## Principles (decision lens)

When choosing between designs, prefer solutions that:

- **Preserve security boundaries**: capability-based authority, least privilege, no ambient authority.
- **Use one coherent model**: reuse security/policy/identity architecture across product surfaces.
- **Keep kernel minimal**: move policy, routing, parsing, and crypto orchestration to userland.
- **Stay performance-scalable**: optimize fast paths, avoid copies, use VMO/filebuffer for bulk transfer.
- **Balance efficiency and burst**: default energy-efficient paths, explicit peak modes.
- **Remain testable**: host-first proofs + minimal authoritative QEMU E2E.
- **Avoid lock-in**: no desktop-only or IoT-only forks of core authority model.
- **Prefer local/self-hostable control paths** where equivalent security can be maintained.
- **Do not clone reference systems 1:1**: adopt principles, keep implementations Rust-idiomatic.

## Architecture stance (current direction)

- **Control plane**: Cap’n Proto IDL in userland (typed messages).
- **Data plane**: VMO/filebuffer for large payloads (low/zero copy).
- **Local IPC**: kernel-enforced endpoint capabilities (RFC‑0005).
- **Policy**: `policyd` decides authority; kernel enforces capability rights.
- **Discovery graph**: `samgrd` registers and resolves services.
- **Distributed (future)**: `softbusd` remains userland-layered; kernel ABI stays local and portable.

## Hybrid security-root roadmap (default)

Use a hybrid approach:

- **MVP trust root**: verified boot + signed bundles + policy gating + capability enforcement.
- **Pluggable key custody**: `keystored`/`identityd` migrates from software bring-up to secure hardware without ABI churn.
- **Measured boot + attestation (later)**: additive trust layer for distributed mesh admission, with trust policy in userland.

## Distributed vision (softbusd, later)

Long-term direction is a userland-driven “device mesh”:

- `softbusd` provides discovery, secure sessions, and routing so services can appear local across devices.
- Kernel IPC remains local and capability-based; distributed behavior is expressed in userland and
  is policy-gated.

## What agents should do by default

- Interpret user decisions and requests **in relation to this vision**.
- If a request seems to conflict with the vision (security/perf/testability), call it out early.
- Propose **better implementations** when they are clearly aligned with the vision, with concrete tradeoffs.
- Keep stubs honest: label them, never fake success markers, and always provide proof of real behavior.
- Keep responses concise by default; expand only when risk, tradeoff, or ambiguity requires detail.
