# nx CLI v1 (host-first)

`nx` is the canonical DevX entrypoint for host tooling in Gate J (`production-floor`).

## CONTEXT

- Scope: `TASK-0045` host-first tooling contract (`RFC-0043`).
- Canonical proof command: `cargo test -p nx -- --nocapture`.

## Quickstart

```bash
# run tests/proofs
cargo test -p nx -- --nocapture

# scaffold
cargo run -p nx -- new service demo

# inspect bundle directory
cargo run -p nx -- inspect nxb path/to/bundle.nxb --json
```

## Exit-code classes

- `0`: success
- `2`: usage / argument parse error
- `3`: validation / security reject
- `4`: required dependency missing
- `5`: delegated command failed
- `6`: unsupported in current repo/runtime state
- `7`: internal io/state error

## Command reference

- `nx doctor [--json]`
  - checks required tools: `rustc`, `cargo`, `just`, `qemu-system-riscv64`, `capnp`
  - returns non-zero (`4`) when required tools are missing
- `nx new service|app|test <name> [--root <path>] [--json]`
  - fail-closed rejects: traversal, separators, absolute paths
  - creates deterministic stub tree and prints manual workspace next-step
- `nx inspect nxb <path> [--json]`
  - summarizes `manifest.*`, `payload.elf` sha256, and `meta/*`
- `nx idl list [--root <path>] [--json]`
  - lists sorted schema names (`*.capnp`)
- `nx idl check [--root <path>] [--json]`
  - validates schema inventory readability and required `capnp` dependency
- `nx postflight <topic> [--tail <N>] [--json]`
  - topic is strict allowlist mapping to existing `tools/postflight-*.sh`
  - uses delegate exit code as truth
- `nx dsl fmt|lint|build [<args...>] [--json]`
  - delegates to backend in `NX_DSL_BACKEND`
  - returns `unsupported` (`6`) when backend is not configured/available

## Postflight topic extension contract

1. Add a new topic key in the static allowlist map in `tools/nx/src/lib.rs`.
2. Map that key to a fixed executable path under `tools/`.
3. Add positive and reject tests (`unknown-topic` and delegate non-zero path).
4. Keep shell injection impossible: never build delegate command via string interpolation.

## Subcommand extension contract

Future topics (`nx config`, `nx policy`, `nx crash`, `nx sdk`, `nx diagnose`, `nx sec`) must extend `tools/nx` as subcommands. Do not introduce separate `nx-*` binaries.
