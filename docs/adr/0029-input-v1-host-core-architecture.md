# ADR-0029: input v1 host core architecture

## Status
Accepted

## Context

After `TASK-0056B` / `RFC-0051`, the UI stack had a deterministic visible-input
proof in `windowd`, but it still lacked one reusable host-first authority for:

- USB-HID boot keyboard/mouse parsing,
- touch sample normalization,
- shared base keymaps for later IME extension,
- deterministic key-repeat scheduling,
- deterministic pointer acceleration.

Without a dedicated architecture record, follow-up work in `TASK-0253` and
`TASK-0146` risks forking parser logic, keymap tables, or repeat/accel behavior
into service-local implementations.

## Decision

Adopt a dedicated host-first input-core layer with these rules:

1. **Single reusable authority**
   - `userspace/hid/`, `userspace/touch/`, `userspace/keymaps/`,
     `userspace/key-repeat/`, and `userspace/pointer-accel/` are the shared
     host input-core authority for parsing, normalization, mapping, repeat, and
     acceleration semantics.
   - Later `inputd` / IME slices must consume or extend these crates rather
     than duplicating their logic.

2. **Routing authority stays in `windowd`**
   - `TASK-0252` does not own hit-test, hover, focus, click, or keyboard target
     selection.
   - `windowd` remains the authority for routed UI input state; 0252 only
     supplies deterministic transport-neutral primitives.

3. **Host-first proof is canonical**
   - The primary proof surface is `tests/input_v1_0_host/`.
   - `cargo test -p input_v1_0_host -- --nocapture` is the canonical behavior
     proof for this slice.
   - No `ready/ok` marker contract exists for 0252 closure.

4. **Fail-closed untrusted input**
   - malformed HID/touch inputs must reject with stable classes,
   - invalid repeat/accel configs must reject instead of silently clamping,
   - locale/environment probing is forbidden for keymap authority.

5. **Transport-neutral for follow-up service wiring**
   - Host core APIs stay transport-neutral so `TASK-0253` can wire QEMU / OS
     input sources without changing parser or keymap contracts.

## Current State

- `TASK-0252` is `Done`.
- Host core crates are landed:
  - `userspace/hid/`
  - `userspace/touch/`
  - `userspace/keymaps/`
  - `userspace/key-repeat/`
  - `userspace/pointer-accel/`
- The canonical host proof package is landed as `tests/input_v1_0_host/`.
- Live OS/QEMU input ingestion, `inputd` service wiring, and `nx input` remain
  follow-up scope in `TASK-0253`.

## Consequences

- **Positive**
  - `TASK-0253` and later IME tasks have one shared input-core authority.
  - Parser, keymap, repeat, and accel behavior can be proven on host without
    relying on QEMU markers.
  - `windowd` authority boundaries remain explicit and anti-drift.

- **Negative**
  - Additional doc/header sync is required because new host crates now form a
    durable architectural surface.
  - Layout coverage and reject behavior must be maintained centrally rather than
    patched ad hoc in downstream consumers.

- **Risks**
  - If follow-up tasks reintroduce service-local keymaps or parser helpers, the
    authority boundary will drift.
  - If tests only mirror implementation shortcuts instead of real behavior,
    host-first closure can still become fake-green.

## Links

- `docs/rfcs/RFC-0051-ui-v2a-visible-input-cursor-focus-click-contract.md`
- `docs/rfcs/RFC-0052-input-v1_0a-host-hid-touch-keymaps-repeat-accel-contract.md`
- `tasks/TASK-0252-input-v1_0a-host-hid-touch-keymaps-repeat-accel-deterministic.md`
- `tasks/TASK-0253-input-v1_0b-os-hidrawd-touchd-inputd-ime-hooks-selftests.md`
- `tasks/TASK-0146-ime-text-v2-part1a-imed-keymaps-host.md`
- `docs/adr/0028-windowd-surface-present-and-visible-bootstrap-architecture.md`
