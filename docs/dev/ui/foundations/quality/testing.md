<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# UI Testing

UI testing is **host-first** with deterministic goldens. QEMU tests are bounded smoke checks.

Performance note:

- UI scenes with glass, overlays, transitions, and animation are valid bounded performance gates.
- Follow `docs/dev/ui/foundations/quality/performance-philosophy.md` when deciding what should be cached, refreshed, or degraded.

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

## TASK-0054 host renderer snapshots

TASK-0054 uses a host-only proof floor for the narrow BGRA8888 CPU renderer:

```bash
cargo test -p ui_renderer -- --nocapture
cargo test -p ui_host_snap -- --nocapture
cargo test -p ui_host_snap reject -- --nocapture
```

The renderer proof asserts expected pixels, 64-byte stride alignment, exact buffer
length, deterministic damage behavior, and stable reject classes. The snapshot
harness compares canonical BGRA pixels against repo-owned goldens under
`tests/ui_host_snap/goldens/`; PNG files are deterministic artifacts only, and
metadata such as gamma or iCCP chunks must not affect equality.

Golden updates are disabled by default. Regeneration must be an explicit
`UPDATE_GOLDENS=1` action, and fixture paths must stay under the snapshot
fixture root. TASK-0054 does not use OS/QEMU present markers.
