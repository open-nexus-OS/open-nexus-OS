# Deprecated: init-lite

This app was an early minimal init used to validate UART markers and cooperative yielding.
It is superseded by `source/init/nexus-init`, which provides both Host (std server) and OS-lite backends.

Use `nexus-init` for all init responsibilities. Keep this directory for historical reference
until removal after the os-lite backend reaches parity.

- See ADR: `docs/adr/0001-runtime-roles-and-boundaries.md`
- Invariants: UART markers remain unchanged; host path is byte-compatible.


