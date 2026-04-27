<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# `nx` CLI

`nx` is the canonical host developer CLI for Open Nexus OS. Extend this binary
as subcommands; do not add `nx-*` helper binaries or duplicate command logic in
scripts.

## Contract Anchors

- DevX guide: `docs/devx/nx-cli.md`
- CLI convergence task: `tasks/TASK-0045-devx-nx-cli-v1.md`
- Config v1: `docs/rfcs/RFC-0044-config-v1-configd-schema-layering-2pc-host-first-os-gated.md`
- Policy as Code v1: `docs/rfcs/RFC-0045-policy-as-code-v1-unified-policy-tree-evaluator-explain-dry-run-learn-enforce-nx-policy.md`
- Structured data decision: `docs/adr/0021-structured-data-formats-json-vs-capnp.md`

## Current Shape

- `src/lib.rs` is a thin entry/test module.
- `src/cli.rs` owns the `clap` argument shape.
- `src/error.rs` owns exit classes and `NxError`.
- `src/output.rs` owns human/JSON output envelopes.
- `src/runtime.rs` owns repo/env-derived runtime paths.
- `src/commands/` owns command implementations.

Keep command behavior deterministic and process-boundary friendly. JSON output
must stay machine-parseable through the common output envelope.

## Implemented Command Families

- `nx new ...`
- `nx inspect ...`
- `nx idl ...`
- `nx postflight ...`
- `nx doctor ...`
- `nx dsl ...`
- `nx config ...`
- `nx policy ...`

## Policy Command Notes

- `nx policy validate` requires `policies/manifest.json` and rejects missing or
  stale manifests fail-closed.
- `nx policy explain` delegates evaluation semantics to `userspace/policy`.
- `nx policy mode` is host preflight-only. It validates authorization and stale
  version preconditions, but it does not mutate a live daemon mode until a real
  `policyd` mode RPC exists and is proven.

## Proof

Run the canonical CLI proof suite:

```bash
cargo test -p nx -- --nocapture
```

For changes touching Config or Policy contracts, also run the corresponding
authority suites:

```bash
cargo test -p nexus-config -- --nocapture
cargo test -p configd -- --nocapture
cargo test -p policy -- --nocapture
cargo test -p policyd -- --nocapture
```

## Extension Rules

- Add new topics as subcommands under `src/commands/`.
- Reuse `ExitClass`, `NxError`, `print_result`, and `RuntimeConfig`.
- Add unit tests in `src/lib.rs` for command behavior.
- Add process-boundary contract tests in `tests/cli_contract.rs` for important
  JSON/exit/file-effect contracts.
- Keep shell delegation allowlisted and argument-vector based; never build
  delegate commands through string interpolation.
