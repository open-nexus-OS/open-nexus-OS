# Boot & bring-up pipeline (onboarding)

This page explains how the system gets from **QEMU boot** to a **validated smoke run**.

Canonical truth for “what must appear in UART logs” is the marker contract:

- `scripts/qemu-test.sh` (authoritative marker ordering + required markers)
- `docs/testing/README.md` (methodology + how to run)

## Why “bring-up” is structured this way

We do not treat “it printed something” as proof. Instead we rely on:

- **Deterministic UART markers** (stable strings, no timestamps/randomness)
- **Marker-driven early exit** (`RUN_UNTIL_MARKER=1`)
- **Bounded runs** (`RUN_TIMEOUT=…`)

This makes QEMU usable in CI and keeps feedback loops tight.

## The boot chain (high level)

At a high level the stack looks like:

0. **First-stage loader** (`nxboot`, `source/boot/nxboot` — ADR-0059, TASK-0289)
   - The VMM `-kernel` payload since the A4 boot flip; a static PIE that runs
     where the firmware loads it (RFC-0098 C6). Reads the device tree first
     (console, memory map, transports), chooses the kernel window (the lowest
     free 2 MiB-aligned window of the first bank — 0x8040_0000 on `virt`),
     reads the BSB double block from the GPT disk, selects the boot slot
     (trial decrement BEFORE load), verifies the slot's NXBD signature against
     the build-baked `policies/os-trust.toml` anchor plus the rollback floor
     and the streamed image sha256, carries the measured handoff record in
     `/chosen/nexus,boot-record` of its copy of the tree and chains into the
     verified image, itself a static PIE that fixes itself up at that window.
   - The kernel reads its platform (console, PLIC, timer, memory banks) from
     the tree in `a1` and injects a read-only alias of it into init's slot 2;
     init discovers the virtio transports and the RTC from it and every device
     capability it grants carries the node's window AND interrupt line
     (`init: devices from fdt ok (…)`, RFC-0098 C3). Each ECAM host of the tree
     is a device source too: init plans its root bus with `nexus-pci` (the
     planner nxboot shares), every BAR on pages of its own, INTx routed, bus
     mastering left to the grant (`init: devices from pci ok (…)`, TASK-0246 P3).
   - The disk is the medium the boot came from: nxboot takes the first candidate
     — virtio block transports, SD hosts in the tree, SD hosts behind each ECAM
     host (planned with the planner init runs) — that carries a valid BSB, reads
     and writes it through the virtio reader or the SDHCI core in PIO
     (TASK-0246B), and records it in `/chosen/nexus,boot-disk` (a node path, or
     `<ECAM host>/mmc@<dev>,<func>` for an SD host behind PCI); init resolves
     that record to the ONE grant `blkd` gets, asked for the kind's class, and
     makes it while only policyd runs, so `blkd` starts with its disk in place
     (`init: boot disk ok (…)`, `blkd: backend ok (…)`, TASK-0246 P4b).
   - The boot trace (RFC-0107): everything the loader prints is also kept on
     that disk, in the `trace` partition's slot for this boot (`nxboot: trace
     slot=<n> seq=<m>`), written when the disk is found, before the jump and on
     every terminal failure; `/chosen/nexus,trace` hands the slot to the OS. A
     board without a serial adapter is read back with `just board-log`; every
     QEMU lane checks that the trace equals the UART's loader lines.
   - Markers: `nxboot: bsb ok …` → `nxboot: verify ok …` → `nxboot: jump
     slot=<s>`; any failure is a stable `nxboot: verify FAIL (…)` /
     `nxboot: PANIC (…)` + SBI reset — never a silent boot of unverified
     bytes. Direct-kernel dev boots stay available (`NEXUS_DIRECT_KERNEL=1`)
     and are honestly labeled `neuron: boot handoff absent (direct kernel)`.
1. **Kernel boot wrapper** (`neuron-boot`)
   - Provides `_start`, clears `.bss`, installs trap vector, jumps into kernel `kmain`.
2. **Kernel** (`source/kernel/neuron`)
   - Brings up HAL, memory map, trap handling, syscalls, scheduler, IPC routing, and selftests.
   - Captures the nxboot handoff page pre-SATP and reports
     `KSELFTEST: boot handoff ok (measured)` (qemu-soft-root measurement).
3. **Userspace init** (`source/init/nexus-init`, os-lite backend)
   - Orchestrates starting services and emits ordered `init: …` markers.
4. **Service daemons** (`source/services/*d`)
   - Register with the service manager, expose IDL, emit `*: ready` markers.
5. **Selftest client / smoke checks**
   - Exercises key contracts end-to-end (policy allow/deny, exec path, VFS, networking/DSoftBus milestones as tasks add them).

## Who owns which markers

To avoid drift and fake success:

- **Kernel** owns low-level bring-up markers (MMU/SATP safety, KSELFTEST markers, etc.).
- **Init** owns `init: start <svc>` (spawn requested) and `init: up <svc>` (the service’s `@ready` observed — RFC-0093 §2); `init: start`/`init: ready` are order-checked, `init: up` presence-checked.
- **Services** own `*: ready` markers once they are genuinely ready to serve requests.
- **Harness** (`scripts/qemu-test.sh`) owns the acceptance criteria: presence + ordering + required subsets.

## Typical dev workflow

On the host:

- Build and run:
  - `make build`
  - `make run`
- Smoke tests (canonical):
  - `RUN_UNTIL_MARKER=1 just test-os` (wraps `scripts/qemu-test.sh`)

For faster QEMU triage (RFC‑0014 Phase 2), you can stop after a named phase:

- Example: `RUN_PHASE=bring-up just test-os`
- Example: `RUN_PHASE=policy just test-os`

When QEMU fails, the harness reports `first_failed_phase=<name>` and prints a bounded UART excerpt scoped to that phase.

If you touch boot sequencing or service bring-up:

- Update the harness expectations in `scripts/qemu-test.sh` **only** when the behavior really changed.
- Update `docs/testing/README.md` if the contributor workflow changes (new required services, new marker tiers).

## Drift-resistant rule of thumb

If you find yourself pasting a long list of markers into a doc:

- Don’t. Put the list in **one place** (the harness) and link to it.
