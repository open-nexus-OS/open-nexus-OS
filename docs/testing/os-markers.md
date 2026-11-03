---
schema: 1
title: "OS markers"
role: reference
subsystem: devx
sources: [source/apps/selftest-client/proof-manifest/markers, source/apps/selftest-client/proof-manifest/phases.toml, tools/nx/chains/markers.txt, scripts/qemu-test.sh, scripts/check-chain-markers.sh, config/fail-marker-allow.txt]
contracts: [RFC-0013, RFC-0014, RFC-0038, RFC-0108]
verified: 2026-11-03
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# OS markers

> **Purpose** — where the UART marker truth lives, how a marker is declared, which gates judge a run, and how to find a marker.
> **Owns** — the reference only; the data is `source/apps/selftest-client/proof-manifest/` and `tools/nx/chains/markers.txt`.
> **Contracts** — [RFC-0014](../rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md) (phases), [RFC-0038](../rfcs/RFC-0038-selftest-client-production-grade-deterministic-test-architecture-refactor-v1.md) (manifest as the one marker source), [RFC-0013](../rfcs/RFC-0013-boot-gates-readiness-spawn-resource-v1.md) (readiness), [RFC-0108](../rfcs/RFC-0108-ux-latency-contract-instant-and-smooth-as-numbers.md) (judged measurement lines)
> **Invariants** — no fake green (CLAUDE.md `no-fake-green`): `*: ready` / `SELFTEST: * ok` only after the real behavior, stubs print `stub`/`placeholder`. A marker string is a stable contract. This page copies no marker lists.
> **Read next** — [proof-manifest.md](proof-manifest.md), [run-logs.md](run-logs.md), `.claude/skills/boot-proof/SKILL.md`

## Where the truth lives

| Source | Holds | Read by |
|---|---|---|
| `source/apps/selftest-client/proof-manifest/markers/*.toml` | every marker literal, its phase, `proves`, profile gates | `selftest-client` build (`M_*` constants), `nexus-proof-manifest` CLI, `scripts/qemu-test.sh`, `tools/os2vm.sh`, `nexus-evidence` |
| `source/apps/selftest-client/proof-manifest/phases.toml` | the 12 RFC-0014 phases and their order | the same |
| `source/apps/selftest-client/proof-manifest/profiles/*.toml` | which lane expects what (`emit_when`/`forbidden_when` resolve against these) | the same |
| `tools/nx/chains/markers.txt` | cross-service chain markers, by group | `cargo test -p nx`, `scripts/check-chain-markers.sh` |
| `scripts/qemu-test.sh` | lane-only ladders (`USB_MARKERS`, `USB_INPUT_MARKERS`, `LOWMEM_INPUT_MARKERS`, …) and `RUN_PHASE` slices | the harness itself |
| `config/fail-marker-allow.txt`, `config/fail-marker-allow-board.txt` | tolerated `FAIL` lines, each with its tracking reference | FAIL gate |

## Phases

<!-- gen:markers -->
| # | phase | markers | declared in |
|---|---|---|---|
| 1 | `bringup` | 578 | board.toml, bringup.toml, ota.toml, ui.toml, usb.toml |
| 2 | `ipc_kernel` | 27 | ipc_kernel.toml |
| 3 | `mmio` | 15 | mmio.toml |
| 4 | `routing` | 53 | routing.toml |
| 5 | `ota` | 73 | ota.toml |
| 6 | `policy` | 54 | policy.toml |
| 7 | `exec` | 71 | exec.toml |
| 8 | `logd` | 56 | logd.toml |
| 9 | `vfs` | 29 | vfs.toml |
| 10 | `net` | 80 | net.toml |
| 11 | `remote` | 21 | remote.toml |
| 12 | `end` | 182 | board.toml, end.toml, ui.toml, usb.toml |

1239 markers in 15 files.
<!-- /gen:markers -->

## Declaring a marker

1. Add the entry to the phase's file under `markers/`:
   ```toml
   [marker."dsoftbusd: transport selected tcp"]
   phase = "net"
   proves = "QUIC path actually used (TCP fallback banned under quic-required)"
   forbidden_when = { profile = "quic-required" }
   ```
   Optional keys: `emit_when`, `emit_when_not`, `forbidden_when` (`{ profile = "<name>" }`,
   matched along the `extends` chain), `introduced_in`. Literals match as substrings of a UART
   line, so one ending in a space or `(` acts as a prefix for a line that carries values.
2. `cargo build -p selftest-client` regenerates `markers_generated.rs` with
   `pub(crate) const M_<KEY>: &str`.
3. Emit through the constant: `emit_line(crate::markers::M_<KEY>)`. `just arch-gate` (Rule 3 in
   `scripts/check-selftest-arch.sh`) fails on a hand-written `SELFTEST:`/`dsoftbusd:` literal
   outside `markers.rs` / `markers_generated.rs`. Services other than `selftest-client` print
   their lines themselves; the manifest still declares them.
4. Changing or renaming a marker: update the emitter, the manifest, `scripts/qemu-test.sh` (if a
   lane ladder names it), `tools/nx/chains/markers.txt` + the chain contract (if it is a chain
   marker), and docs, in one change.
5. Check: `cargo test -p nexus-proof-manifest`;
   `cargo run -p nexus-proof-manifest -- list-markers --profile=<p>` prints the expected ladder.

Markers are ASCII. `dbg:` lines are never markers. A guest writes no line per keystroke or per
frame (privacy rule, `docs/standards/SECURITY_STANDARDS.md`).

## Gates on a run

| Gate | Where | Fails when | Off switch (manual only) |
|---|---|---|---|
| Ladder | `scripts/qemu-test.sh` (`list-markers --profile`) + lane ladders | a required marker is missing; prints `first_failed_phase=` and `missing_marker=` | — |
| Deny-by-default | `nexus-proof-manifest verify-uart` | an unexpected or `forbidden_when` marker appears (all lanes except the non-display list in `profile_has_display` and `display-gpu`) | `PM_VERIFY_UART=0` |
| FAIL gate | `scripts/qemu-test.sh` | any `SELFTEST:`/`KSELFTEST:` `FAIL` line not in `config/fail-marker-allow.txt` | `FAIL_MARKER_GATE=0` |
| Chain contract | `scripts/check-chain-markers.sh` | a chain group of `tools/nx/chains/markers.txt` the lane covers is missing | `MARKER_CONTRACT=0` |
| Trace contract | `scripts/qemu-test.sh` (RFC-0107) | the boot disk's kept trace differs from the UART | — |
| Pixel proof | `tools/pixel_proof_judge.py` (`visible*` lanes) | the desktop or settled frame is black, has an unpainted band, or equals the splash | — |
| Evidence seal | `scripts/qemu-test.sh` post-pass | assemble/seal fails when sealing is required ([evidence-bundle.md](evidence-bundle.md)) | `NEXUS_EVIDENCE_DISABLE=1` (rejected under `CI=1`) |

Check a chain group against any log: `just check-markers build/logs/<run>/uart.log` or
`bash scripts/check-chain-markers.sh --log <uart.log> --groups input-live`.

## Kinds of lines

- **Readiness** — `<svc>: ready` (serves its contract) vs `init: up <svc>` (init saw `@ready`).
- **Proof** — `SELFTEST: … ok` / `KSELFTEST: … ok`, with a matching `FAIL` form where a probe can fail.
- **Explicit absence** — a lane without a device prints why, e.g. `init: usb plane none (no host
  controller)`; the ladder requires the honest absence line, not silence.
- **Measurement** — numbers without a verdict (e.g. `KSELFTEST: ipc stats (`); the ladder pins the prefix only.
- **Chain hops** — `<svc>: chain <X><n> …`, once per boot per hop, in groups the contract names
  (`input-route` I5–I6, `input-live` I1–I2, `input-chord` K1–K4: a keyboard chord from hidrawd
  through inputd to windowd on the key channel). A `hops` line times a hop with numbers only, e.g.
  `inputd: hops chord (code= report>push=`; on the board the last hop present names where a
  human's input stopped.
- **Judged measurement** (RFC-0108) — `<service>: ux <name> <ok|warn|FAIL> (… budget_<unit>=<n>)`.
  Budgets live only in `source/libs/nexus-ux-budgets/src/lib.rs`; the rung is the prefix
  (`gpud: ux frame `), the verdict is judged by the FAIL scan and its allow-lists.
- **Build provenance** — gpud's first line `gpud: features=…` names the compiled feature set;
  `scripts/check-bundle-provenance.sh` checks the bundle for it.

Interactive boots (`just start`) fold routine lines into per-service verdicts and expand only
failures (RFC-0068); proof boots print every line raw. Headless GPU coverage is
`just ci-os-display-gpu-pci`; the visible desktop is `just start` or `just start-vnc`
(judge a frame with `tools/visual-postflight.py`).

## Finding a marker

```bash
# Which phase/file declares it, what it proves, which profiles gate it
grep -rn -A4 'marker."SELFTEST: policy allow ok"' source/apps/selftest-client/proof-manifest/markers/
# Who emits it
grep -rn 'M_SELFTEST_POLICY_ALLOW_OK' source/apps/selftest-client/src/
# Is it a chain marker or in a lane ladder
grep -n 'policy allow ok' tools/nx/chains/markers.txt scripts/qemu-test.sh
# The full expected ladder of one profile
cargo run -q -p nexus-proof-manifest -- list-markers --profile=smp1
# Did the last run print it
grep -a 'SELFTEST: policy allow ok' "$(ls -dt build/logs/*--*/ | head -1)uart.log"
```
