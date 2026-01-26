<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# `nx dsl` CLI

This CLI is designed to be deterministic and host-first.

## Core commands

```bash
nx dsl fmt --check
nx dsl lint
nx dsl build               # emits canonical .nxir
nx dsl build --emit-json   # also emits derived .nxir.json
nx dsl snapshot            # renders headless goldens (v0.1b+)
```

## Generators (optional)

Keep the default scaffold minimal. Expand structure only when needed:

```bash
nx dsl init <appdir>
nx dsl add page <Name>
nx dsl add component <Name>
nx dsl add store <Name> --scope session
nx dsl add store <Name> --scope durable
nx dsl add service <Name>
nx dsl add test unit <target>
nx dsl add test component <target>
nx dsl add test e2e <target>
```

## Session helpers (optional)

For host runs and debugging:

```bash
nx dsl session inspect
nx dsl session clear
nx dsl session export --json
```

Notes:

- “Session state” is in-memory by default.
- “Durable state” should use typed snapshots (`.nxs`) via settings/app-state, not an untyped JSON file.
