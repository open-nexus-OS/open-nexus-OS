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
