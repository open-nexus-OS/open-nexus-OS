---
name: boot-proof
description: Run a QEMU proof lane for this repo and decode the result — profiles, UART marker ladder, hypothesis.json triage. Use when a change needs boot-level proof or when a QEMU run failed and you need to diagnose it.
---

# Boot proof (QEMU marker ladder)

The proof of OS behavior is the UART marker ladder, never a green exit code
alone. One command per lane; each run writes `build/logs/<profile>--<ts>/`.

## Run a lane

```bash
just ci-os-headless        # full service chain, no display (default CI lane)
just ci-os-smp             # SMP=2 strict gate + SMP=1 parity
just ci-os-display-gpu-pci # GPU pipeline proof via UART markers
just ci-network            # dhcp + quic + os2vm aggregate
just start                 # interactive visible boot (virgl window, not a gate)
```

Profiles and their required markers are declared in
`source/apps/selftest-client/proof-manifest/` (harness.toml + markers/), and
enforced by `scripts/qemu-test.sh`.

## Read the result

1. `build/logs/latest/uart.log` — the boot transcript. Grep the marker the
   harness reported as missing; `build/logs/latest` can be stale, prefer the
   newest `<profile>--<ts>` dir by timestamp.
2. `build/logs/latest/hypothesis.json` — NDJSON triage records; decode
   `hypothesisId` via `build/logs/README.md` (H4 = build errors,
   H4b = per-service compiler warnings, H5 = gate steps).
3. `qemu.stderr` / `build.stderr` in the same dir for harness-level failures.

## Rules

- A missing marker is a real regression until proven otherwise — do not
  "fix" a lane by weakening its marker set.
- Marker strings are stable contracts: changing one requires updating
  `scripts/qemu-test.sh`, `tools/nx/chains/markers.txt`, and docs together.
- Black screen in visible lanes is a compositor/gpud issue in OUR stack —
  never diagnose toward host X11/Wayland/VNC.
- Bounded debugging: 2-3 hypotheses per run, check
  `docs/testing/README.md` troubleshooting before guessing.
