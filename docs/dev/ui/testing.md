<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Testing

UI testing is **host-first** with deterministic goldens. QEMU tests are bounded smoke checks.

## Golden snapshots

Use headless rendering + deterministic fixtures.

```bash
# Example (tooling varies by task/lane)
nx dsl snapshot <appdir> --route / --profile desktop --locale en-US
```

Guidance:

- Prefer stable fixtures (no wallclock, no RNG).
- Keep goldens small and representative.
- Any “ok” marker must correspond to real behavior (no fake success).
