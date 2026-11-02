---
schema: 1
id: RFC-XXXX
title: "The contract in one line"
status: draft
owners: ["@runtime"]
created: YYYY-MM-DD
subsystem: kernel
tasks: [TASK-XXXX]
depends_on: []
links:
  - label: ADR
    target: docs/adr/XXXX-….md
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# RFC-XXXX: The contract in one line

## Why

The problem today, measured, and why it needs a contract rather than a task.

## Scope

- **Owns:** the interfaces, invariants and decisions this RFC defines.
- **Does not own:** what belongs to other RFCs or to the tasks (link them).

## Contract (normative)

The interface, wire format, ABI or protocol; the error model; the versioning rule. If something
is not stable yet, say so.

## Invariants

What must always hold — determinism, bounds on every buffer, loop and queue, the security floor —
and the `test_reject_*` that proves each.

## Failure model

Every error condition and the required behaviour. No silent fallback: a fallback is explicit and
proven.

## Security (when relevant)

Threat model · mitigations · DON'T DO · open risks.

## Proof

The host tests and the markers the tasks must make green:

```bash
cargo test -p <crate> <filter>
just test-os <profile>
```

## Alternatives considered

Each with why it was rejected.
