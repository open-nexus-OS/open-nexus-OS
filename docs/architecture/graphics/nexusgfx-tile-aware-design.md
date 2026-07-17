# NexusGfx Tile-Aware Design

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for future mobile/GPU backends

---

## Purpose

This document captures the **mobile/tile-aware performance posture** for
`NexusGfx`, especially relevant if Open Nexus OS targets an Imagination-like
GPU architecture.

The goal is not to document one vendor's exact hardware, but to shape the SDK
and pass model around the realities of:

- tile-local memory,
- bandwidth sensitivity,
- transient attachments,
- and pass-locality.

---

## Why this matters

Desktop-style graphics intuition often optimizes for:

- abundant external memory bandwidth,
- large discrete GPU memory pools,
- and immediate-mode-style rendering assumptions.

That is the wrong default for many mobile-class GPUs.

On tile-aware architectures, the dominant win often comes from:

- avoiding external memory traffic,
- keeping intermediate data local,
- reducing store/load churn,
- and cutting unnecessary resolves/flushes.

---

## Architectural assumption

`NexusGfx` should assume that a likely real GPU backend is:

- bandwidth-sensitive,
- tile-aware,
- and rewarded by explicit pass structure.

This does **not** force all backends to behave identically. It means the API and
artifact model should preserve enough information for a mobile/tile-aware
backend to be efficient.

---

## Core rules

### 1. Bandwidth-first over brute force

Prefer designs that reduce external memory traffic even if they require:

- more explicit pass structure,
- more careful attachment lifetime tracking,
- or tighter staging discipline.

### 2. Transient attachments are first-class

If an attachment does not need to survive beyond a bounded pass region, the
architecture should allow it to be declared transient.

### 3. Passes should preserve locality

The pass model should make it possible to:

- keep intermediate values local,
- avoid unnecessary pass fragmentation,
- and avoid redundant store/load cycles.

### 4. Hidden barriers are harmful

Overly conservative or hidden dependency handling can destroy tiling benefits by
forcing flushes at the wrong time.

---

## Store/load posture

The architecture should preserve explicit intent for:

- whether an attachment needs a previous value loaded,
- whether a result must be stored,
- whether the result is transient only,
- whether a resolve is required,
- and whether the pass output is consumed immediately by a following phase.

This is useful for all backends, but especially valuable for mobile GPUs.

---

## Transient attachment posture

Good candidates for transient resources:

- G-buffer-like intermediates
- depth/stencil-like scratch data
- MSAA intermediates
- temporary lighting/composition targets
- tile-local postprocess intermediates

Do not automatically allocate long-lived external storage for these by default.

---

## Pass planning rules

The planner should prefer:

- bounded passes with meaningful locality,
- no unnecessary split between producer and immediate consumer stages,
- no attachment persistence when the value is not reused,
- no hidden full-frame resolve when a bounded resolve is enough.

The planner should avoid:

- pass fragmentation caused by convenience rather than necessity,
- one-pass-per-effect architectures that thrash memory,
- and generic desktop-style assumptions that ignore tile locality.

---

## What hurts tile-aware efficiency

These patterns should be treated as anti-patterns in later tasks:

- redundant full-surface store/load cycles
- unnecessary resolves
- attachment oversubscription
- barriers inserted without real dependency need
- forcing every postprocess into an externally stored texture
- treating compute as a universal replacement for localized pass fusion

---

## CAD and pro-app notes

Tile-aware design still matters for pro workloads:

- viewports and overlays should remain bandwidth-conscious,
- selection/highlight buffers should not force excessive full-frame churn,
- timeline/preview surfaces should reuse transient or partial resources where
  possible,
- and large-scene rendering should stream resources while keeping pass-local
  work bounded.

The "authoritative data" may live on CPU, but the interactive surface should
still respect mobile bandwidth constraints.

---

## CPU backend relationship

The CPU backend will not prove tile-memory behavior directly.

But it can and should prove:

- pass boundaries,
- store/load intent,
- transient vs persistent resource declarations,
- and that the API preserves enough information for a tile-aware backend to act
  efficiently later.

That is the right host-first proof posture.

---

## First milestone guidance

The first milestone should make the following explicit, even if only the CPU
backend exists:

- transient attachments exist in the model,
- pass boundaries are explicit,
- store/load/resolve intent is explicit,
- and validation/profiling can see these distinctions.

This keeps the first slice small while preventing later mobile-backend drift.

---

## Related

- `docs/architecture/nexusgfx-command-and-pass-model.md`
- `docs/architecture/nexusgfx-resource-model.md`
- `tasks/TRACK-NEXUSGFX-SDK.md`
