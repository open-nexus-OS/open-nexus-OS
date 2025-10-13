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
