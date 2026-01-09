---
title: TRACK Drivers & Accelerators (GPU/NPU/VPU/Audio/Camera/ISP/Storage/Net/Sensors): contracts + gated roadmap
status: Living
owner: @runtime
created: 2025-12-22
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - IPC/caps model: docs/rfcs/RFC-0005-kernel-ipc-capability-model.md
  - Hardware/MMIO access: tasks/TASK-0010-device-mmio-access-model.md
  - VMO plumbing: tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md
  - QoS/timers: tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md
  - Audit sink: tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md
  - UI consumer (renderer abstraction): tasks/TASK-0169-renderer-abstraction-v1a-host-sceneir-cpu2d-goldens.md
  - UI consumer (windowd wiring): tasks/TASK-0170-renderer-abstraction-v1b-os-windowd-wiring-textshape-perf-markers.md
  - NexusGfx SDK track (consumer + future GPU backend): tasks/TRACK-NEXUSGFX-SDK.md
  - ADR (DriverKit ABI policy): docs/adr/0018-driverkit-abi-versioning-and-stability.md
  - Extracted (DriverKit core v1): tasks/TASK-0280-driverkit-v1-core-contracts-queues-fences-buffers.md
  - Extracted (DMA buffer ownership prototype): tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md
---

## Goal (track-level)

Provide a coherent, minimal, and extensible foundation for **device-class services**
(GPU/NPU/VPU/Audio/Camera/ISP/Storage/Networking accelerators/Sensors)
that prioritizes:

- bounded memory + deterministic behavior,
- low/zero-copy data plane (VMO/filebuffer),
- capability-gated device access with audit,
- crash containment (driver services can crash/restart without kernel crash),
- a path to high-performance “pro” profiles and power-efficient defaults.

## Non-Goals

- This file is **not** an implementation task.
- No QEMU markers are required for this track document itself.
- No kernel changes are defined here (kernel work gets its own tasks).

## Contracts (stable interfaces to design around)

- **Device access**: cap-gated “device broker” model (MMIO/IRQ/DMA handles).  
  Source: `tasks/TASK-0010-device-mmio-access-model.md`
- **Buffers/images**: VMO/filebuffer handles, RO sealing, slices, budgets.  
  Source: `tasks/TASK-0031-zero-copy-vmos-v1-plumbing.md`
- **Sync**: timeline fences + waitsets + deadlines (avoid busy-wait).  
  Source: `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md` (QoS/timers)
- **Submit/queues**: bounded in-flight work, backpressure, QoS hints (Frugal/Normal/Burst).
- **Fault/reset**: per-client kill + device reset semantics, audited.
- **DriverKit / SDK boundary (cross-device)**:
  - The *same* “submit + fence + buffers + budgets” concepts must work across GPU, NPU, ISP, VPU, and even audio.
  - Device-specific code should mostly be:
    - register/MMIO programming,
    - firmware protocol (if required),
    - command stream encoding/validation,
    - reset/recovery.
  - Everything else (resource model, scheduling, budgets, tracing hooks) should live in shared, versioned userland libraries.
- **Policy**: `policyd` decides, kernel enforces rights on held caps.  
  Source: `docs/rfcs/RFC-0005-kernel-ipc-capability-model.md`
- **Observability**: logd/journal + structured audits.  
  Source: `tasks/TASK-0006-observability-v1-logd-journal-crash-reports.md`

## Gates (RED / YELLOW / GREEN)

- **RED (blocking)**:
  - **Safe userspace device access**: without a real MMIO/IRQ/DMA capability model, userspace drivers cannot be real.  
    Gate: `TASK-0010`.
  - **DMA isolation**: to treat vendor blobs as “untrusted”, we eventually need IOMMU/GPU-MMU style isolation (later kernel/hardware work).
- **YELLOW (risky / drift-prone)**:
  - **Read-only sealing semantics**: library convention vs kernel-enforced RO mappings must be explicitly documented per phase.
  - **Clock domains**: display/audio/video device clocks need a coherent timing story for low jitter and power savings.
  - **Vendor plugin boundaries**: keep the “driver kit” ABI narrow and versioned; supply-chain policy must gate loading.
- **GREEN (confirmed direction)**:
  - Data plane should use VMO/filebuffer for bulk payloads (Vision).
  - Services should be process-isolated (process-per-service).

## Phase map (what “done” means by phase)

- **Phase 0 (bring-up, minimal real behavior)**:
  - device online + buffer import + submit + fence completes
  - hard bounds (bytes/time) and clear failure modes
- **Phase 1 (robust + power-aware)**:
  - backpressure end-to-end, timeouts, reset/recovery
  - QoS hints integrated and observed in behavior
- **Phase 2 (performance/features)**:
  - richer pipelines (e.g., GPU raster), advanced scheduling, optional features (e.g., RT later)

## Backlog (Candidate Subtasks)

These are *not* tasks yet; they become real `TASK-XXXX` items only when they can be proven deterministically.

- **CAND-DRV-000: DriverKit core (cross-device)**  
  - **Status**: extracted → `TASK-0280`

- **CAND-DRV-010: GPU device-class service skeleton (brokered MMIO + command validation)**  
  - **What**: a GPU driver service pattern (cap-gated device handles, command buffer validation, reset/recovery hooks) without requiring real hardware yet  
  - **Depends on**: `TASK-0010` (MMIO/IRQ/DMA access model), `CAND-DRV-000`  
  - **Proof idea**: host tests (command validation) + QEMU marker for “device open/close/reset path” (no fake rendering claims)  
  - **Status**: candidate

- **CAND-DRV-020: Audio device-class service contract (buffers + timeline sync + QoS)**  
  - **What**: unify audio “submit + fence + buffer” semantics with the DriverKit model to keep UI/audio sync deterministic  
  - **Depends on**: `TASK-0013`, `CAND-DRV-000`  
  - **Proof idea**: host tests for timeline fence scheduling and bounded jitter model (stub sink)  
  - **Status**: candidate

- **CAND-DRV-030: Camera/ISP pipeline skeleton (VMO frames + deadlines + privacy gates)**  
  - **What**: a camera-style frame graph contract (VMO frames, metadata side channel, deadlines, fault containment)  
  - **Depends on**: `TASK-0031`, policy/consent tasks (runtime prompts + indicators)  
  - **Proof idea**: host tests for frame graph determinism; QEMU markers for “frame produced” using fixtures  
  - **Status**: candidate

- **CAND-DRV-090: Track integration list (consumers)**  
  - **What**: keep a short list of major consumers that must remain compatible with DriverKit contracts  
  - **Examples**: renderer/windowd, media, privacy indicators, DSoftBus share streams  
  - **Status**: candidate (documentation-only)

See also:

- Networking driver sub-track: `tasks/TRACK-NETWORKING-DRIVERS.md`

## Extraction rules

- Only extract a candidate into a real `TASK-XXXX` when it has:
  - deterministic proof (host tests and/or QEMU markers where meaningful),
  - explicit “minimal v1” vs “future deluxe” boundaries,
  - and no contract drift (no new ad-hoc formats/schemes without a decision task).
- After extraction, keep only a link and `Status: extracted → TASK-XXXX`.
  - If the extracted task defines a stable boundary, add/update the relevant ADR (e.g. DriverKit ABI policy).
