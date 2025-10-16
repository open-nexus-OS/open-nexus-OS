# Service lifecycle notes

## Service startup order (scaffold)

The init process currently brings up services in the following sequence while
the platform is under construction:

- `init`
- `keystored`
- `policyd`
- `samgrd`
- `bundlemgrd`
- `init: ready`

Each stub emits a `*: ready` marker on the UART. `nexus-init` prints matching
`*: up` confirmations as it observes readiness so the QEMU harness can enforce
the order deterministically.

The kernel banner marker to expect in logs is `neuron vers.` rather than `NEURON`.

## Execution pipeline

Service launch now flows through the policy gate:

1. `bundlemgrd` exposes each installed bundle's capability requirements via
   `QueryResponse.requiredCaps`.
2. `nexus-init` asks `policyd.Check` whether the service may consume those
   capabilities. Denials are logged as `init: deny <name>` and the service is
   skipped.
3. When `policyd` approves, init forwards the request to `execd` which performs
   the actual launch (stubbed today to emit `execd: exec <name>`).

This pipeline applies to every non-core service defined under `recipes/services/`.

### Planned bundlemgrd → execd handoff (v1.1)

The next revision of the execution flow teaches `bundlemgrd` to vend an ELF
image for a named service. The daemon will either hand back a read-only VMO or
forward the raw bytes for `execd` to stage. `execd` will then reuse the vended
handle when wiring the loader, avoiding today’s embedded-payload bootstrap.

## Minimal exec path (MVP)

`execd` now exposes `exec_minimal(subject)` as a bootstrap-friendly path while
the real ELF/NXB loaders are being implemented. The handler:

1. carves a temporary, shared stack from a static buffer (the kernel still runs
   all tasks in a single address space at this stage),
2. reuses slot `0` of its capability table as the bootstrap endpoint, seeding
   the child with send-only rights via `cap_transfer`,
3. issues the `spawn` syscall targeting the `hello_child_entry` payload from
   `userspace/exec-payloads`, and
4. logs `execd: spawn ok <pid>` once the kernel acknowledges the request.

The child prints `child: hello` and yields forever, proving that the task was
scheduled. `selftest-client` emits `SELFTEST: e2e exec ok` after invoking the
new handler so the QEMU harness can assert the full sequence. The shared stack
and address space are strictly temporary; per-task address spaces land in the
next milestone.

## Loader v1 (PT_LOAD only)

The first loader revision lives in the `nexus-loader` crate. It consumes a
RISC-V ELF64 image, validates the header, enforces W^X, and plans mappings for
each `PT_LOAD` segment. On the OS build `execd::exec_elf_bytes` wires the plan
as follows:

1. Allocate a fresh address space with `as_create` and stage the bundle bytes in
   a scratch VMO.
2. Feed the ELF bytes through `nexus_loader::load_with` using the OS mapper. The
   mapper aligns each segment, calls `as_map`, and preserves W^X semantics.
3. `StackBuilder` provisions a private stack with a guard page, copies `argv`
   and `env` tables, and reports the new stack pointer back.
4. `execd` invokes `spawn(entry, sp, as_handle, bootstrap_slot)`, then seeds the
   child with a send-only copy of slot `0`.

The child receives a `BootstrapMsg` on slot `0` (kernel populated for now) and
enters at the ELF entry point with its dedicated stack. QEMU smoke tests watch
for the markers `execd: elf load ok`, `child: hello-elf`, and
`SELFTEST: e2e exec-elf ok` to prove the path end-to-end. Later revisions will
replace the embedded payload with bundlemgrd-delivered VMOs.
