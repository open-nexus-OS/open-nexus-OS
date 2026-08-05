---
title: "TRACK: network proof lanes — `just ci-network` is red (dsoftbus cross-device + profile/runner wiring)"
status: Active
owner: "@runtime"
created: 2026-08-05
links:
  - Playbook: CLAUDE.md
  - Testing index: docs/testing/README.md
  - Phase contract: docs/rfcs/RFC-0014-testing-contracts-and-qemu-phases-v1.md
  - Cross-VM task (marked Done, see W1): tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md
  - Marker contract: source/apps/selftest-client/proof-manifest/markers/net.toml
  - Profile manifest: source/apps/selftest-client/proof-manifest/profiles/harness.toml
---

## Why this track exists

`just ci-network` (= `ci-os-dhcp` + `ci-os-quic` + `ci-os-os2vm`) has been red
since **at least 2026-07-24** and nobody noticed. It is in neither `test-all`
nor `.github/workflows/ci.yml`, so no gate looks at it.

That is the meta-finding worth keeping: the `ci-parity` gate added on
2026-08-05 (`scripts/check-ci-parity.sh`) asserts that everything CI runs is
reachable from `test-all` — but `ci-network` is in **neither**, so it falls
through that net too. A lane nothing gates will rot silently.

Measured 2026-08-05 on `1ac92652`, all on host-built artifacts:

| lane | result |
| --- | --- |
| `ci-os-dhcp` | green |
| `ci-os-quic` | red — see W2 |
| `ci-os-os2vm` | red — runner fixed (W3), underlying defect is W1 |

## W1 — dsoftbus cross-device discovery never happens (the real defect)

`just test-os2vm` (the declared 2-VM harness, `tools/os2vm.sh`) fails with:

```
[wait] cross-vm markers pending: A=0 B=0 elapsed=176s remaining=4s
[info] Phase done: discovery status=failed duration_ms=179057
[summary] result=failed code=OS2VM_E_DISCOVERY_TIMEOUT phase=discovery node=both
```

Neither node emits a single cross-VM marker in the full 180 s window — this is
not a slow handshake, discovery does not occur at all. Consistent with
`SELFTEST: dsoftbus os connect FAIL` appearing in **every** run log back to
2026-07-24 (single-VM profiles tolerate it; only `REQUIRE_DSOFTBUS=1` lanes
enforce it).

`TASK-0005` (cross-VM sessions + remote proxy) is marked **Done**. Either that
closure regressed or the proof lane that guarded it stopped being run. Resolving
W1 should reconcile that ledger's status rather than leave "Done" standing
against a lane that cannot pass.

Not started. Owns the actual fix; W2/W3 are wiring around it.

## W2 — `quic-required` demands a peer it cannot have

`[profile.quic-required]` (harness.toml) declares:

```toml
runner = "scripts/qemu-test.sh"     # SINGLE-VM harness
env = { REQUIRE_DSOFTBUS = "1" }
```

`REQUIRE_DSOFTBUS=1` makes `scripts/qemu-test.sh` require
`SELFTEST: dsoftbus os connect ok` and `SELFTEST: dsoftbus ping ok` — a
cross-device connect. The marker contract itself documents that this is
impossible in that topology (`markers/net.toml`, on
`dsoftbusd: quic msg1 timeout`):

> the single-VM quic handshake probe gave up waiting for msg1
> (**no peer in single-VM profiles**)

So the profile asserts something it cannot satisfy by construction.

The likely intent is narrower and already expressed by the `forbidden_when`
markers on that profile (`dsoftbusd: transport selected tcp`,
`dsoftbus: quic os disabled (fallback tcp)`, `SELFTEST: quic fallback ok`):
*if* a session is established, it must be QUIC and never the TCP fallback.
Under that reading `REQUIRE_DSOFTBUS=1` should be dropped and the peer-requiring
assertions left to the 2-VM lane.

**Deliberately not changed.** Markers are a stable contract (CLAUDE.md
`no-fake-green`): changing what a profile asserts means moving
`scripts/qemu-test.sh`, `tools/nx/chains/markers.txt` and the docs together, and
it is a decision about what the lane is *for* — not a drive-by edit.

## W3 — the declared `runner` is documentation only (hardening, deferred)

**Bug fixed 2026-08-05**: `ci-os-os2vm` ran `RUN_OS2VM=1 just test-os os2vm`,
and `test-os` invokes `scripts/qemu-test.sh` — which does not reference
`RUN_OS2VM` anywhere. The variable was set and silently ignored, so the lane
booted ONE VM and then demanded cross-VM markers that cannot exist there,
failing for a fabricated reason and masking W1. It now delegates to
`test-os2vm`, which runs the declared harness.

**The structural hole behind it is deferred, not fixed.** Nothing enforces the
manifest's `runner` field: `scripts/qemu-test.sh` never reads it, so any recipe
can route any profile through the wrong harness and only a human will notice.

The repo already has the right pattern for this one layer down. Profile env is
enforced as SSOT with a hard stop on conflict (`pm_apply_profile_env`,
`scripts/qemu-test.sh`) — introduced after a caller-supplied `SMP=1` was
silently clobbered and `ci-os-smp1`, the "deterministic boot gate", was really
running the 2-hart MTTCG lane. `runner` deserves the same treatment:

- `nexus-proof-manifest` already models it (`runner: Option<String>` with
  `extends` inheritance, `source/libs/nexus-proof-manifest/src/lib.rs:105`) but
  the CLI exposes only `list-env` and `list-markers`.
- Add a `runner` subcommand, then have `scripts/qemu-test.sh` hard-stop when the
  resolved runner is not itself, naming the correct one.

That makes the drift impossible rather than remembered. Touches
`source/libs/**` (ABI-stability zone), hence a deliberate decision rather than a
side effect of the W3 bug fix.

## Exit criteria

- [ ] W1: `just test-os2vm` reaches cross-VM discovery and the `os2vm` marker
      group passes; `TASK-0005` status reconciled against the evidence.
- [ ] W2: `quic-required` asserts something it can satisfy, with the marker
      contract, `scripts/qemu-test.sh` and docs moved together.
- [x] W3 (bug): `ci-os-os2vm` uses the runner its profile declares.
- [ ] W3 (hardening): a profile routed through the wrong runner is a hard stop,
      derived from the manifest.
- [ ] Once green, decide where `ci-network` is gated — otherwise it rots again.
      It is currently in neither `test-all` nor CI, and `ci-parity` cannot see
      lanes that are in neither.
