# NexusGfx Artifact Pipeline

**Created**: 2026-04-10  
**Owner**: @ui @runtime @devx  
**Status**: Active architecture guidance for shader/kernel artifact planning

---

## Purpose

This document defines the artifact posture for `NexusGfx` so later tasks can
generate a deterministic, offline-first toolchain without rediscovering the
same constraints.

Artifacts are important because they connect:

- source/IR,
- backend lowering,
- signed outputs,
- runtime loading,
- warmup,
- and profiling/debugging.

---

## Core stance

`NexusGfx` should be **artifact-first**, not runtime-JIT-first.

The preferred default is:

- offline compile or prepare,
- deterministic artifact generation,
- stable IDs and hashes,
- signed or otherwise integrity-checked outputs,
- bounded runtime warmup/caching.

On-device compilation may exist later, but it should be:

- optional,
- bounded,
- and not a requirement for the first milestone.

---

## Artifact classes

The architecture should distinguish at least:

- **shader source inputs** or equivalent frontend definitions
- **IR artifacts**
- **pipeline artifacts**
- **specialized variants**
- **debug symbol/source mapping sidecars**
- **cache indexes/manifests**

For the future portable stack, this should cover:

- graphics pipeline work
- compute kernels
- and future interop with infer/graph artifacts where shared infrastructure is useful

---

## Stable identity

Artifacts need stable identifiers so runtime caches and tooling can reason about
them without guesswork.

Recommended identity inputs:

- canonicalized source or IR
- stable configuration fields
- explicit capability/profile requirements
- specialization constants or equivalent bounded knobs
- target artifact format version

Do not let:

- filesystem traversal order,
- temporary paths,
- host timestamps,
- or nondeterministic map iteration

affect artifact identity.

---

## Offline-first compilation posture

The first-class pipeline should be:

1. source/IR is generated deterministically
2. artifact IDs are computed deterministically
3. artifacts are compiled/generated offline or during a controlled build step
4. runtime loads and validates prepared artifacts

This is the right posture for:

- deterministic builds
- stable perf behavior
- reduced runtime stalls
- smaller TCB than uncontrolled JIT everywhere

---

## Signed output posture

Signed outputs matter for:

- supply-chain hygiene
- artifact provenance
- backend compatibility checks
- and safe distribution/update stories

The first milestone does not need a full production-grade signing universe, but
the architecture should assume:

- pipeline artifacts can be integrity-checked,
- artifacts carry stable version info,
- and runtime rejects incompatible or malformed blobs deterministically.

---

## Runtime warmup and pipeline caching

The runtime may keep a bounded cache of prepared pipelines/artifacts.

Required posture:

- stable cache keys
- bounded cache size
- explicit warmup stages where useful
- no unbounded background harvesters
- no runtime mutation that breaks reproducibility

Potential future extensions:

- profile-driven warmup
- app-install-time prewarming
- captured "harvested" variants from controlled execution

---

## Specialization posture

The architecture should allow bounded specialization where it provides clear
value.

Examples:

- format/attachment variants
- text/material variants
- tile-aware path variants
- capability-driven compute variants

But specialization must remain:

- explicit,
- bounded,
- and reflected in artifact identity.

Avoid generating an explosion of near-duplicate variants by default.

---

## Dev Studio relationship

This artifact pipeline should later feed `TRACK-DEVSTUDIO-IDE.md` naturally.

Dev Studio should be able to:

- inspect artifact IDs
- show compile diagnostics
- prebuild artifacts
- package/sign them
- deploy them deterministically

That is easier if the artifact architecture is defined before IDE tasks expand.

---

## Debugging and profiling relationship

Artifacts should carry enough stable identity for:

- pipeline labels in traces
- reproducible perf scenes
- debug symbol/source mapping
- deterministic reproduction of compile/runtime issues

This should work even when the actual backend implementation differs between CPU
reference and future hardware.

---

## First milestone guidance

The first milestone does not need a huge compiler stack.

It should lock:

- stable artifact IDs
- deterministic artifact generation rules
- offline-first posture
- integrity/version checks
- bounded runtime cache behavior

That is enough to make later tasks and RFCs concrete.

---

## Related

- `tasks/TRACK-NEXUSGFX-SDK.md`
- `tasks/TRACK-DEVSTUDIO-IDE.md`
- `docs/architecture/nexusgfx-compute-and-executor-model.md`
