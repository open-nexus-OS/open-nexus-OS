---
schema: 1
title: Vision
role: guide
subsystem: devx
sources: [source/kernel/neuron, source/services]
contracts: [RFC-0005, ADR-0001]
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Vision

> **Purpose** — the product and architecture direction every change is judged against.
> **Owns** — no code; the decision lens for `source/kernel/neuron/` and `source/services/`
> **Contracts** — [RFC-0005](../rfcs/RFC-0005-kernel-ipc-capability-model.md) (kernel IPC and capabilities), [ADR-0001](../adr/0001-runtime-roles-and-boundaries.md) (one implementation per role)
> **Invariants** — capability-based authority, no ambient authority; a minimal kernel; stubs are labelled and never emit success markers.
> **Read next** — [system-map.md](system-map.md), [README.md](README.md)

## North star

A **secure, high-performance OS core** that scales across phone, desktop, TV, automotive and
embedded classes.

- **RISC-V-first, Rust-first**: Rust-idiomatic designs, stable interfaces, simple invariants.
- **Fast by default**: low-latency interaction with energy-aware defaults.
- **Peak mode when explicit**: burst performance for pro workloads without a power-hungry baseline.
- **Local sovereignty**: local development and self-hosted operation where policy allows.
- **Managed posture**: family, school, enterprise and fleet policies share one authority model.
- **Distributed fabric**: services feel local across trusted devices through userspace layers.

## Principles (decision lens)

Prefer designs that:

- **preserve security boundaries** — capabilities, least privilege, no ambient authority;
- **use one coherent model** — the same security, policy and identity architecture on every surface;
- **keep the kernel minimal** — policy, routing, parsing and crypto orchestration in userspace;
- **stay performance-scalable** — fast paths, few copies, VMOs for bulk data;
- **balance efficiency and burst** — efficient defaults, explicit peak modes;
- **remain testable** — host-first proofs, minimal authoritative QEMU runs;
- **avoid lock-in** — no desktop-only or IoT-only fork of the authority model;
- **adopt principles, not clones** — learn from reference systems, keep implementations our own.

## Architecture stance

- **Control plane**: Cap'n Proto IDL in userspace (typed messages).
- **Data plane**: VMOs for large payloads (low or zero copy).
- **Local IPC**: kernel-enforced endpoint capabilities (RFC-0005).
- **Policy**: `policyd` decides authority; the kernel enforces capability rights.
- **Discovery**: `samgrd` registers and resolves services.
- **Distributed**: `dsoftbusd` stays in userspace; the kernel ABI stays local and portable.

## Security root

- **Trust root**: verified boot, signed bundles, policy gating, capability enforcement.
- **Pluggable key custody**: `keystored` / `identityd` move from software keys to secure hardware
  without ABI churn.
- **Measured boot and attestation**: an additive layer for admitting devices to the mesh, with trust
  policy in userspace.

## Direction

- **Managed devices.** Remote systems may supply management inputs — enrollment, group roles,
  policy bundles, config profiles, rollout intent — but local authorities (`policyd`, `configd`,
  `identityd`, `bundlemgrd`) validate, store, explain and enforce them. No remote bypass around
  policy and no second install authority.
- **Scripts and automation.** Shell scripts and package-manager hooks are allowed and expected, but
  run bounded and auditable: project-scoped, no silent machine-wide mutation, no service
  registration or broad filesystem and network access without an explicit grant. "Script can run"
  never means "script owns the machine".
- **Device mesh.** `dsoftbusd` provides discovery, secure sessions and routing so services can
  appear local across devices; kernel IPC remains local and capability-based, and distributed
  behavior is policy-gated in userspace.

## What agents do by default

- Read requests against this vision; call out early when one conflicts with security, performance
  or testability.
- Propose a better implementation when it clearly fits the vision, with concrete tradeoffs.
- Keep stubs honest: label them, never emit a success marker for them, prove real behavior.
