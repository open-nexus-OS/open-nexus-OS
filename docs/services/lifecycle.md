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
