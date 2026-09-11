---
title: TASK-0325 Proof lanes declare their resource envelope, never die silently, and CI owns the full ladder's verdict
status: Done
owner: @tools-team @runtime
created: 2026-09-11
depends-on: []
follow-up-tasks: []
links:
  - ADR: docs/adr/0063-proof-lane-resource-envelope-and-authority.md
  - Harness: scripts/qemu-test.sh, source/apps/selftest-client/proof-manifest/profiles/harness.toml
  - Logs: docs/testing/run-logs.md, docs/testing/README.md
  - Trigger: tasks/TASK-0324-display-handoff-deterministic-by-construction.md (P4e-2 proof set, killed 4x)
---

# TASK-0325: A proof lane accounts for its own resources and always leaves a witness

## Why

Four lane runs in a row died during TASK-0324 P4e-2's proof set with
`error: recipe test-os was terminated by signal 15` — an external supervisor reacting to
low system-wide free memory. The run directories stop mid-stream and `hypothesis.json`
says **nothing** about it: a killed lane is indistinguishable from a hang, a QEMU crash or
a wedged guest. The harness that enforces "no marker without behaviour, no green without a
witness" across the whole OS was itself producing verdicts no one can read.

Measured with cgroup accounting (`ota-bundle-delta`, build + QEMU in one scope):
`memory.peak = 845 MB`; with `MemoryHigh=4G`/`MemoryMax=7G` in force, `memory.events`
reported `high 0, max 0, oom 0`. So the lane is not the consumer — the machine was full of
desktop processes, one of them a file indexer walking the lane's own build output. Two
conclusions, both acted on here: an envelope cannot stop an external killer (only ADR-0063
rule 3 can), and nothing today bounds or even RECORDS what a lane consumes, so a lane that
did regress would look exactly like this failure.

## Packages

| # | Package | Content | Status |
|---|---------|---------|--------|
| P0 | A lane never dies silently | `TERM`/`INT`/`HUP` trap in `scripts/qemu-test.sh` → `qemu-test: <half> terminated externally (signal N)` on stderr + a `hypothesis.json` record (signal, elapsed, phase, profile, `half` = supervisor/lane); deliberately NOT written into `uart.log` (that file is the guest's word); every enveloped run records `memory.peak`/`memory.current`/limits/`memory.events` | Delivered 2026-09-11 — proven: `timeout 240` SIGTERM → both halves wrote the witness + records |
| P1 | A lane declares its envelope | `NEXUS_LANE_MEM_HIGH=4G/_MEM_MAX=8G` on `[profile.full]` (CPU/IO weights supported, declared only with evidence — see finding 6) (inherited by every harness profile through `extends`); `qemu-test.sh` re-execs into a transient user cgroup scope; the breach verdict comes from the cgroup's own `memory.events` (`max`/`oom_kill`), never from an exit code; unenforceable (no user cgroup scope) is SAID, never pretended | Delivered 2026-09-11 — proven: forced breach (`MemoryMax=64M`) → `resource envelope EXCEEDED … hit the wall 32x, 1 OOM kill(s)`; normal smp1 lane green, `enforced=true`, peak 726 MB, zero events |
| P2 | Derived output marks itself | `build/CACHEDIR.TAG` written by `scripts/build.sh` (cargo already does this for `target/`); `check-build-truth.sh` checks the CODE (build.sh still marks the dir) and the signature of an existing tag — never the machine state; indexer exclusions documented as developer setup | Delivered 2026-09-11 — proven: removing the call → FAIL; no tag on disk → still PASS |
| P3 | CI owns the verdict | ADR-0063 rule 3 recorded in `docs/testing/README.md`; workstation runs what fits its machine | Delivered 2026-09-11 (ADR-0063 Accepted, testing README section) |
| P4 | Proof | envelope breach forced → witness; external kill → witness; normal lane green with its peak recorded; cold `make clean` + `test-all` in a measurement scope for the cold-build number ADR-0063 lists as unmeasured | Done 2026-09-11 — breach/kill/normal proven; cold `make clean` + `just test-all` GREEN in ONE run (rc=0, 3,361 s, all 8 lanes exit 0, zero kills): cold compile anon peak 1,768 MiB, measurement-scope `memory.peak` 9,670 MiB (mostly page cache), lane peaks 751-1,099 MiB, zero envelope events anywhere |

## Findings (recorded 2026-09-11)

1. **The lane is not the consumer.** cgroup accounting: `ota-bundle-delta` 845 MB, `smp1` 726 MB
   (build + QEMU in one scope, guest `-m 320M`). With 4G/8G in force, `memory.events` stayed at
   zero. The pressure that killed four runs was the desktop session (a file indexer at 1.2-1.3 GB
   growing with every build, mail and browser ~1.3 GB, the agent session ~1.16 GB). An envelope
   therefore cannot prevent an external kill on global pressure — rule 3 of ADR-0063 exists for that.
2. **A snapshot outside the lane's own scope measures the machine, not the lane.** The first
   version recorded the supervisor's cgroup too — the enclosing terminal tab — and reported a
   7.07 GB "lane" peak. A record labelled as the lane's cost that measures something else is worse
   than no record: snapshots are now taken only inside the lane's own scope.
3. **The breach verdict cannot key on an exit code.** The designed `137` (SIGKILL) was measured as
   `143` (SIGTERM) — the service manager tore the scope down before the OOM killer's kill was the last word. The
   cgroup's `memory.events` (`max 32-41`, `oom_kill 1`) is what says an envelope was exceeded.
4. **The first derived-dir gate was machine-shaped.** "`build/` exists without CACHEDIR.TAG → FAIL"
   would have turned `just check` red on every existing checkout until a build ran. The gate now
   checks the code (the marking call) and the content of a tag that exists.
5. **`make clean` is not the memory problem it looks like.** The cold compile's non-reclaimable
   (`anon`) peak is 1,676 MB; its `memory.peak` (9,865 MB) is mostly page cache the kernel returns
   on demand — zero envelope events in the whole cold `test-all`.
6. **CPU/IO weights are policy, not measurement — removed from the default envelope.** The
   first envelope also declared `CPUWeight=50`/`IOWeight=50`. A cold run then failed a lane
   whose guest runs on host wall-clock time (no icount) on a polling defect (TASK-0324 P7
   carry-over). Whether the weights contributed is NOT proven — an earlier `ota-flip` passed
   with them — but lowering a proof lane's CPU share under contention can only make
   timing-sensitive lanes less reliable, and nothing measured asked for it. The memory limits
   are the measured part and stay.
7. **The ratchet admitted one new file, on purpose.** Consolidating the selftest client's three
   rngd exchanges moved 6 positional declarations in 2 files into 2 in one new file
   (`services/rngd.rs`); the baseline was regenerated because both totals shrank (134/46 →
   130/45). The route migrates onto the topology with the selftest client (TASK-0324 P4f).

## Not in this task

CI hardware/runner selection; containerising the local build; tuning tools this repo does
not own (indexers, browsers); per-developer machine configuration.
