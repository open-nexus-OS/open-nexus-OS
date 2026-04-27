# NexusGfx Sync and Lifetime Model

**Created**: 2026-04-10  
**Owner**: @ui @runtime  
**Status**: Active architecture guidance for future `NexusGfx` tasks/RFCs

---

## Purpose

This document defines the synchronization and lifetime posture for `NexusGfx`.

The goal is explicit, bounded, portable semantics for:

- queue submission,
- pass completion,
- resource ownership transfer,
- present pacing,
- and future graphics/compute/infer interop.

---

## Core rule

Synchronization must be:

- **explicit**
- **bounded**
- **portable**
- **visible in tests and profiling**

The architecture should avoid:

- hidden global synchronization,
- unbounded waits,
- busy loops,
- and backend-specific magic that changes behavior silently.

---

## Queue ownership

Queues are the unit of submission ordering.

Required posture:

- submission order is explicit,
- in-flight depth is bounded,
- queue ownership is clear,
- and backpressure is observable.

The queue model should work for:

- CPU reference,
- future GPU execution,
- optional compute queues,
- and future infer interop.

---

## Fence model

Timeline-style thinking should be the default architecture stance even if some
early implementations use simpler forms internally.

Fences should express:

- work completion,
- dependency sequencing,
- ownership return,
- and present pacing.

Important uses:

- resource reuse after submit
- CPU readback after device work
- present synchronization
- infer/gfx interop handoff

TASK-0055 closeout note:

- `windowd` v1b implements only a minimal headless `PresentAck`
  (`PresentSeq` + damage count) after checked composition.
- That acknowledgement is a deterministic proof/fence surrogate for the
  headless slice, not a latency-accurate GPU/display fence.
- Visible scanout, richer present pacing, and resource-reuse fences remain
  follow-up scope.

---

## Wait model

Wait semantics must be bounded.

Preferred classes:

- non-blocking check
- blocking wait with deadline/timeout
- waitset/group wait for bounded collections

Do not allow:

- unbounded "wait forever" surfaces in the portable contract,
- hidden spin loops in userspace,
- or timing-dependent fake-green tests.

This aligns with existing timed/deadline posture in `TASK-0013`.

---

## Resource ownership transfer

The resource model and synchronization model must work together.

Required posture:

- CPU owns a resource until submit/import transfers usage,
- resource reuse before completion must be rejected or staged safely,
- fence completion returns or authorizes reuse,
- imported resources need explicit ownership handoff rules.

This is the graphics-side analog of the DMA ownership work in `TASK-0284`.

---

## Lifetime states

The architecture should distinguish at least these conceptual states:

- `created`
- `recorded-for-use`
- `in-flight`
- `completed`
- `recycled`
- `destroyed`

For imported resources:

- `imported-visible`
- `imported-in-flight`
- `returned`

Why:

- avoids accidental use-after-submit,
- makes validation deterministic,
- gives profiling and debugging a stable vocabulary.

---

## Pass and submission dependencies

Dependencies should be modeled in terms of:

- pass ordering,
- queue ordering,
- resource access intent,
- fence/wait requirements.

The portable contract should not require apps to reason about vendor-specific
barrier packet formats, but it **must** keep dependencies explicit enough to:

- validate misuse,
- avoid hidden flushes,
- and preserve tile-aware/locality opportunities.

---

## Present pacing

Present integration is not "just another copy"; it is a synchronized endpoint.

The model should expose:

- frame-ready completion,
- pacing deadlines,
- dropped-frame accounting,
- bounded frame queue depth,
- and presentable resource lifetime.

This is critical for:

- games,
- SystemUI,
- CAD navigation,
- video preview,
- and low-jitter pro surfaces.

---

## Fault and reset posture

The runtime must assume backend faults can happen.

Required posture:

- failed submits produce stable error classes,
- in-flight resources become recoverable only through explicit reset semantics,
- queue/device reset is audited,
- per-client fault containment is preferred over global collapse.

Do not assume:

- "the backend will always recover transparently",
- or that lost work can be retried without explicit state transition.

---

## Interop synchronization

Future interoperability needs an explicit sync story:

- `NexusGfx` <-> `windowd`
- `NexusGfx` <-> media frames
- `NexusGfx` <-> `NexusInfer`

Interop rules should state:

- who owns the resource,
- which fence signals handoff,
- which queue/pipeline can consume it next,
- and when the producer may reclaim it.

Without this, zero-copy interop becomes "copy until it works".

---

## Validation expectations

The validation layer should be able to reject at least:

- use-after-submit
- double-submit
- queue overflow
- invalid fence waits
- reuse before completion
- imported-resource misuse
- presentable-resource misuse

The diagnostics should explain **why denied** in bounded, deterministic form.

---

## First milestone guidance

The first milestone does not need the full universe of synchronization features.

It should lock:

- queue submission ordering,
- bounded in-flight limits,
- fence completion for ownership return,
- deadline-bounded waits,
- present integration hooks,
- stable error mapping.

That is enough to generate future tasks without ambiguity.

---

## Related

- `docs/architecture/nexusgfx-resource-model.md`
- `docs/architecture/nexusgfx-command-and-pass-model.md`
- `tasks/TASK-0013-perfpower-v1-qos-abi-timed-coalescing.md`
- `tasks/TASK-0284-userspace-dmabuffer-ownership-v1-prototype.md`
