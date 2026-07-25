<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Troubleshooting

QEMU/UART triage tips and determinism knobs. Split out of the former `docs/testing/index.md`; see [README.md](README.md) for the entry point.

For regression hunting across runs, `scripts/regression-bisect.sh` is a manual
(not CI-wired) evidence-replay bisect helper — see
[replay-and-bisect.md](replay-and-bisect.md).

## Troubleshooting tips

- QEMU runs are bounded by the `RUN_TIMEOUT` environment variable (default `45s`). Increase it only when debugging: `RUN_TIMEOUT=120s just qemu`. During kernel bring-up we rely on marker-driven early exit and strict triage: the kernel prints `map kernel segments ok` once linker-derived mappings are active and `AS: post-satp OK` after each SATP switch; illegal-instruction traps print `sepc/scause/stval` and instruction bytes; optional symbolization (`trap_symbols`) resolves `sepc` to `name+offset`; RX-sanity failures panic with the offending PC before the switch.
- CI log filters fail the run on `PANIC`, `EXC:`, `ILLEGAL`, `rx guard:`, or missing marker sequences and retain deterministic seeds plus the trimmed UART/QEMU artefacts for post-mortem analysis.
- Logs are trimmed post-run. Override caps with `QEMU_LOG_MAX` or `UART_LOG_MAX` if you need to preserve more context. Each run drops three auto-generated artefacts (all ignored by Git) in the workspace root: `uart.log`, `uart_ecall_chars.log`, and `neuron-boot.map`. Keep the map around while triaging traps—agents rely on it to resolve `sepc` offsets without rerunning the build. Use `tools/uart-filter.py --strip-escape uart.log` to decode the escape-prefixed probe lines or `--grep init:` to focus on readiness markers.
- Enable marker-driven early exit for faster loops by setting `RUN_UNTIL_MARKER=1` (already defaulted in `just test-os`). Logs appear as `qemu.log` (diagnostics) and `uart.log` (console output) in the working directory. Set `QEMU_TRACE=1` and optionally `QEMU_TRACE_FLAGS=in_asm,int,mmu,unimp` to capture detailed traces while debugging. Enable the `trap_ring` feature to retain the last 64 trap frames in debug builds or `trap_symbols` to annotate `sepc` with `name+offset` when triaging crashes. `scripts/run-qemu-rv64.sh` accepts `NEURON_BOOT_FEATURES=trap_ring,trap_symbols` so you can flip those flags per-run without touching manifests; the helper passes them into the kernel build before QEMU launches. Kernel panic/guard prints remain unconditional (mirroring capability-microkernel panic channels) until we finish the current KPGF work; once the loader is stable we plan to gate them behind a `panic_uart` feature and rely on the trap ring for routine post-mortems.
- Init-lite tags its verbose traces with RFC‑0003-style topics. Export `INIT_LITE_LOG_TOPICS=svc-meta` (comma-separated) before invoking `just qemu`/`just test-os` to opt into additional channels such as the service metadata probes. The build script records the value via `cargo:rustc-env`, so changing it automatically triggers the rebuild already performed by `scripts/run-qemu-rv64.sh`.
- Init-lite emits two unconditional bring-up markers for triage: `!init-lite entry` before any service work and `!init-lite ready` after all configured services spawn. Fatal bootstrap errors log a `fatal err=…` line and then panic the init task so the failure is visible in UART.
- To catch silent spins during bring-up, you can enable a watchdog that panics after N cooperative yields: `INIT_LITE_WATCHDOG_TICKS=500 RUN_TIMEOUT=10s just qemu`. Leave it unset for normal runs.
- Escape-coded UART (E-prefixed) is currently forced off to keep logs clean. Old logs can still be decoded with `tools/uart-filter.py --strip-escape uart.log`. If you need probe framing, add it locally in code, but keep it disabled for shared runs.
- For stubborn host/container mismatches, rebuild the Podman image and ensure the same targets are installed inside and outside the container.

### Dead interactive session (`just start`): desktop at 320x240, no input

Symptom grid from `build/logs/manual--*/uart.log`, in the order it appears:

| Line | Means |
|---|---|
| `windowd: gpud reply foreign frame op=0x49 …` | **`0x49` is `b'I'`** — the first byte of the client-surface envelope `[b'I', b'N', version, op]` (`nexus-display-proto::client_surface`, shared with the input-live ops), NOT a gpud status byte (those are 0/1/2). Something is draining client traffic off an endpoint it does not own and discarding it. |
| no `WINDOWD: app event channel attached` | the desktop's events-attach frame (`len=12`) was one of the discarded ones |
| `WINDOWD: desktop bind deferred (channel not yet attached)` | the deferral is waiting for an attach that was already consumed — it can never complete |
| `apphost: no content rect (fallback)` ~8 s after the intent | the geometry intent (`len=32`) was discarded too |
| `WINDOWD: desktop surface created id=1 320x240` | the app-host fell back to the probe size; the desktop presents frames and never routes input |
| `inputd: push backpressure n=…` + `hidrawd: chain I2 wire send FAIL` | inputd's batches (`len=32`) are being discarded as well |

Root cause (fixed 2026-07-25): windowd's gpud route came from a runtime
`new_for("gpud")` query whose recv side aliased windowd's own server endpoint,
and `drain_gpud_replies` — which runs on every present — consumed the client
frames queued there. The drain now refuses any endpoint that IS windowd's own
inbox (`windowd: FAIL gpud reply endpoint aliases server inbox slot=… — drain
skipped`), and a client-envelope frame that reaches it anyway stops it at once
(`windowd: FAIL gpud reply ate client frame op=… len=…`). Note the route
SELECTION was left alone on purpose: pinning the runtime to the init-wired pair
(slots 5/6) instead breaks the framebuffer handoff in the headless lane
(`gpud: ERROR attach framebuffer resource create failed` → `chain G4 scanout
FAIL`) — so if you see G4 fail after touching `ensure_gpud_client`, that is why.
A `just start`-class regression needs `just start`-class boots to see: this race
never fires in the headless 1–2-hart proof lanes, only at 4 harts + virgl.

### QEMU smoke proof knobs (determinism)

Network/distributed debugging runbooks, packet capture workflow, `os2vm` phase controls, typed error matrix, and slot-mismatch triage are maintained in:

- `docs/testing/network-distributed-debugging.md` (SSOT)

Keep this guide focused on the global testing framework; use the SSOT document for operational network/distributed triage details.

### Test log structure

All test/QEMU runs write to `build/logs/<profile>--<timestamp>/` (the `latest` symlink can be stale). See [`docs/testing/run-logs.md`](run-logs.md) for the run-directory layout, the `hypothesis.json` decode grid, and `just logs-gc [keep]` pruning.
