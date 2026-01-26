<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# DSL Testing

The DSL is **host-first**:

- most correctness is proven via host tests,
- QEMU is a bounded smoke layer with deterministic markers.

## Snapshot testing (v0.1b+)

```bash
nx dsl snapshot <appdir> --route / --profile desktop --locale en-US
```

Conventions:

- inputs/fixtures live under `ui/tests/fixtures/`
- goldens live under `ui/tests/goldens/`
- generated artifacts live under `target/nxir/` and `target/nxir/snapshots/`

## What to test

- parse/format idempotence
- lowering determinism and diagnostics
- reducer purity violations are rejected
- snapshot parity (interpreter vs AOT where applicable)
