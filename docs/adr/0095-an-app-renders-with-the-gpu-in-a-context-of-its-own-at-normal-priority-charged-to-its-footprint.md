# ADR-0095: An app renders with the GPU in a context of its own — at normal priority, bound from its own surfaces, charged to its footprint, granted by its manifest

- Status: Proposed (2026-11-04, TASK-0351 P4 — the contract; the app-host GPU renderer is its consumer task)
- Date: 2026-11-04
- Links:
  - Tasks: `tasks/TASK-0351-layer-8-on-the-gpu-an-occluded-window-costs-nothing-bindings-grow-a-fullscreen-a.md`
  - RFCs: RFC-0101 §1 (contexts), §7b (the library), §6 (reset); RFC-0114 §1 (surfaces are the
    owner's objects); RFC-0100 (the footprint)
  - Related ADRs: ADR-0092 (a surface is a buffer the app draws and the GPU reads), ADR-0067
    (the compositor boundary), ADR-0094 (one kernel, three executors)

## Context

Since layer 5 the board's GPU draws windowd's frames in windowd's context, and windowd alone
may open one (`gpud/gpu_context.rs`, `test_reject_open_from_another_service`). An app draws its
surface on the CPU (the DSL raster) into the buffer the GPU reads (RFC-0114). An app that wants
the GPU — a canvas, a game, a viewer, the DSL's own raster moved onto it — has no path: the
authority to open a context is a policy decision this stack has not made, and the memory such a
context binds is charged to nobody.

## Decision

**An app may open a GPU context of its own.** It is opened at normal priority (windowd's stays
the highest; §6's preemption and timeout apply per context), granted by the capability named in
the app's manifest through app-host's provisioning (fail-closed; the owner is the kernel's
sender id, never a payload field), with a library the app brings and gpud validates exactly as
windowd's (§7b). The context binds surfaces of the app's own (RFC-0114 §1, allocated by gpud for
that owner) and nothing else; what it binds is charged to the app's footprint by memd
(`charges.rs`), so the per-app limit covers the GPU. An owner's death destroys its context with
its bindings (§6's reset path) and frees its surfaces. Until the renderer lands, the refusal of
every sender but windowd stays as it is — this ADR changes the contract, the consumer task
changes the code.

## Consequences

- **Positive**: the GPU reaches apps through the one driver split (RFC-0101) without a second
  path; the footprint rule of layer 8.1 (TASK-0352) covers GPU memory the day an app binds it;
  the DSL's raster can move onto the GPU as one more executor of the kernel tables (ADR-0094).
- **Negative / accepted cost**: a second priority class on the board's GPU is a real scheduling
  question (the firmware's preemption, measured, not assumed — the consumer task's first board
  cycle); a validated library per app costs the validation at open.
- **Follow-ups**: the consumer task (the app-host GPU renderer: a context opened by an app, one
  GPU-rendered frame presented on QEMU virgl and on the board, its footprint read with the GPU
  buffers in it); `policies/base.toml` gains the capability; `test_reject_a_context_beyond_the_app_s_footprint`.

## Alternatives considered

- **Apps submit through windowd's context.** Rejected: one address space for every app's GPU
  memory is one fault domain and no footprint per app.
- **A second device server for apps.** Rejected: two executors of one device, the thing ADR-0094
  removes.
