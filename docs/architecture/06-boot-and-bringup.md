# Boot & bring-up pipeline (onboarding)

This page explains how the system gets from **QEMU boot** to a **validated smoke run**.

Canonical truth for “what must appear in UART logs” is the marker contract:

- `scripts/qemu-test.sh` (authoritative marker ordering + required markers)
- `docs/testing/index.md` (methodology + how to run)

## Why “bring-up” is structured this way

We do not treat “it printed something” as proof. Instead we rely on:

- **Deterministic UART markers** (stable strings, no timestamps/randomness)
- **Marker-driven early exit** (`RUN_UNTIL_MARKER=1`)
- **Bounded runs** (`RUN_TIMEOUT=…`)

This makes QEMU usable in CI and keeps feedback loops tight.

## The boot chain (high level)

At a high level the stack looks like:

1. **Kernel boot wrapper** (`neuron-boot`)
   - Provides `_start`, clears `.bss`, installs trap vector, jumps into kernel `kmain`.
2. **Kernel** (`source/kernel/neuron`)
   - Brings up HAL, memory map, trap handling, syscalls, scheduler, IPC routing, and selftests.
3. **Userspace init** (`source/init/nexus-init`, os-lite backend)
   - Orchestrates starting services and emits ordered `init: …` markers.
4. **Service daemons** (`source/services/*d`)
   - Register with the service manager, expose IDL, emit `*: ready` markers.
5. **Selftest client / smoke checks**
   - Exercises key contracts end-to-end (policy allow/deny, exec path, VFS, networking/DSoftBus milestones as tasks add them).

## Who owns which markers

To avoid drift and fake success:

- **Kernel** owns low-level bring-up markers (MMU/SATP safety, KSELFTEST markers, etc.).
- **Init** owns the `init: start <svc>` / `init: up <svc>` sequencing (orchestration truth).
- **Services** own `*: ready` markers once they are genuinely ready to serve requests.
- **Harness** (`scripts/qemu-test.sh`) owns the acceptance criteria: presence + ordering + required subsets.

## Typical dev workflow

On the host:

- Build and run:
  - `make build`
  - `make run`
- Smoke tests (canonical):
  - `RUN_UNTIL_MARKER=1 just test-os` (wraps `scripts/qemu-test.sh`)

If you touch boot sequencing or service bring-up:

- Update the harness expectations in `scripts/qemu-test.sh` **only** when the behavior really changed.
- Update `docs/testing/index.md` if the contributor workflow changes (new required services, new marker tiers).

## Drift-resistant rule of thumb

If you find yourself pasting a long list of markers into a doc:

- Don’t. Put the list in **one place** (the harness) and link to it.
