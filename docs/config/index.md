<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Config v1 Contract (Host-first)

Config v1 enforces deterministic, fail-closed behavior for authoring, layering, canonicalization, and reload.

## Authority split

- Canonical runtime/persistence snapshot: Cap'n Proto (`tools/nexus-idl/schemas/config_effective.capnp`)
- Authoring/validation and debug/CLI view: JSON (`schemas/*.schema.json`, `nx config ... --json`)

## Layering order

Deterministic precedence is fixed:

`defaults < /system < /state < env`

- `defaults`: in-crate deterministic base model
- `/system/config/*.json`: image defaults merged in lexical order
- `/state/config/*.json`: mutable overrides merged in lexical order
- `nx config push <file>` writes `state/config/90-nx-config.json`
- `NEXUS_CFG_*`: bounded env overlay
- objects deep-merge; lists replace as a unit

## `configd` contract

- `GetEffective`: canonical Cap'n Proto bytes plus deterministic version
- `GetEffectiveJson`: derived JSON view plus the same version
- `Subscribe`: update notifications only after a committed version transition

`nx config effective --json` must align semantically with `configd`'s derived JSON view and version for the same layered inputs.

## Reload semantics

Reload is authoritative and fail-closed:

- prepare reject => abort, previous effective version stays active
- prepare timeout => abort, previous effective version stays active
- commit failure => abort/rollback, previous effective version stays active
- success is reported only after full prepare + commit completes

## Fail-closed reject taxonomy

- `reject.unknown_field`
- `reject.type_mismatch`
- `reject.depth_overflow`
- `reject.size_overflow`
- `reject.list_overflow`

Any reject class blocks apply/reload and keeps the previous effective version active.

## `nx config` surface

- `validate`: schema/type/bounds validation for explicit files or repo layers
- `effective --json`: derived effective JSON plus canonical version
- `diff`: semantic comparison between two candidate overlays
- `push`: validates then writes deterministic state overlay
- `reload`: exposes commit/abort result with version fields
- `where`: prints canonical system/state/env source map

## Troubleshooting

- reject on unknown key: remove fields outside the published schemas under `schemas/`
- reject on type/bounds: fix the authoring JSON; config inputs are not coerced
- reload abort with unchanged version: inspect the reported prepare/commit failure and keep the previous version as truth
- mismatch between human expectations and runtime data: treat Cap'n Proto effective snapshot/version as authority, JSON as derived view only

## Host proof commands

```bash
cargo test -p nexus-config -- --nocapture
cargo test -p configd -- --nocapture
cargo test -p nx -- --nocapture
```

These proofs cover reject behavior, deterministic layering, Cap'n Proto byte determinism, `configd` API alignment, honest 2PC abort/rollback behavior, and `nx config` process-boundary contracts.
