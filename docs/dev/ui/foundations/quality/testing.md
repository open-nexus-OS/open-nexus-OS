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

## TASK-0055 headless windowd present

TASK-0055 adds the first headless `windowd` surface/layer/present proof. It is
still not a visible scanout claim.

```bash
cargo test -p windowd -p ui_windowd_host -p launcher -p selftest-client -- --nocapture
cargo test -p ui_windowd_host reject -- --nocapture
cargo test -p ui_windowd_host capnp -- --nocapture
RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

The host proof asserts desired behavior: two damaged surfaces composite to exact
pixels, no damage skips present, layer ordering is deterministic, minimal
present acknowledgements are emitted only after composition, and invalid or
unauthorized VMO/surface/layer/commit requests fail closed. It also runs the
TASK-0055 Cap'n Proto schemas through generated bindings and roundtrips
surface, queue-buffer damage, scene commit, vsync subscribe, and input subscribe
messages.

The OS proof uses a small deterministic headless desktop profile
(`64x48@60Hz`) to stay within current selftest heap limits. Its markers are
supporting evidence only and are accepted by postflight only through the
canonical QEMU harness and proof-manifest verification.

VMO scope is deliberately narrow here: TASK-0055 proves `windowd` rejects
missing, forged, wrong-rights, wrong-size, or non-surface buffer handles. It does
not claim new kernel VMO capability transfer, sealing/reuse, IPC fastpath, or
zero-copy production behavior.
