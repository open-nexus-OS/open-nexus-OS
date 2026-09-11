# ADR-0063: A proof lane declares its resource envelope, never dies silently, and CI — not a workstation — is the authority for the full ladder

- Status: Accepted
- Date: 2026-09-11
- Links:
  - Tasks: `tasks/TASK-0325-proof-lane-resource-envelope.md` (execution + proof)
  - RFCs: — (harness policy, not a wire contract)
  - Related ADRs: `docs/adr/0062-boot-stage-fence-and-readiness-barriers.md` (the same
    principle one layer down: declared barriers instead of ambient timing)
  - Related: `docs/architecture/02-selftest-and-ci.md`, `docs/testing/run-logs.md`

## Context

A proof lane declares almost everything about itself already: its markers and phases
(`proof-manifest/`), its build inputs (`just inputs`, pinned toolchain), its service
features (`[package.metadata.nexus-service]`), its QEMU topology (`profiles/harness.toml`).
It declares **nothing about what it needs from the machine it runs on**, and takes memory,
CPU and I/O ambiently out of a pool it shares with whatever else the host is doing.

On 2026-09-10/11 that cost four lane runs in a row (TASK-0324 P4e-2's proof set). An
external supervisor observed low system-wide free memory and sent `SIGTERM`; `just` reported
`error: recipe test-os was terminated by signal 15`, and the run directory simply **stopped
mid-stream**. Nothing in `hypothesis.json` said the lane had been killed. A later reader —
human or agent — cannot distinguish that from a hang, a QEMU crash or a wedged guest, which
is precisely the "no fake green / every verdict needs a witness" rule of this repo, violated
by the harness itself.

Measurement (cgroup accounting of a real `ota-bundle-delta` lane, build + QEMU inside one
scope) shows the lane is **not** the consumer: `memory.peak = 845 MB`, and with
`MemoryHigh=4G` / `MemoryMax=7G` set, `memory.events` reported `high 0, max 0, oom 0` — the
limits were never engaged. The machine was full of desktop processes (a file indexer at
1.2 GB, mail and browser at ~1.3 GB combined) — including an indexer whose work is *caused
by the lane's own build output*. So:

- an envelope around the lane cannot stop an external killer that fires on global pressure;
- but nothing today bounds or even records what a lane consumes, so a lane that DID regress
  into eating memory would look exactly like today's failure: an unexplained SIGTERM;
- and the only signal that is independent of the host's other tenants is one produced where
  there are no other tenants.

Production-grade reference systems agree on the last point: one runs hermetic test realms
whose capabilities are explicitly routed, on dedicated infrastructure; another gives each CI
job a fresh, isolated VM and marks derived build output so the system indexer never walks it;
a third builds in a pinned container image and tests on a device farm. In all of them the
developer workstation is a convenience, not the authority.

## Decision

Three rules, in order of load-bearing weight.

1. **A lane never dies silently.** `scripts/qemu-test.sh` traps `TERM`/`INT`/`HUP`, writes a
   `qemu-test: FAIL lane terminated externally (signal N)` line into the run's UART log tail
   and a `hypothesis.json` record naming the signal, the elapsed time and the phase reached.
   Every run — successful or not — records its own `memory.peak`, `memory.current` and the
   limits that were in force. An unexplained end of a run directory is a defect in the
   harness, not a state a reader has to guess about.
2. **A lane declares its resource envelope where it declares everything else.** The profile
   (`profiles/harness.toml`) carries `NEXUS_LANE_MEM_HIGH` and `NEXUS_LANE_MEM_MAX`
   (`NEXUS_LANE_CPU_WEIGHT`/`_IO_WEIGHT` are supported but only declared with evidence — a lane
   without icount runs its guest on host wall-clock time, so lowering its share makes it less
   reliable); `qemu-test.sh` re-executes itself inside a transient user cgroup scope
   carrying exactly those limits. `MemoryHigh` is the
   declared working set (exceeding it reclaims, it does not kill); `MemoryMax` is the wall
   that says "this is a bug" — crossing it kills inside the lane's own cgroup and rule 1
   produces the witness. Numbers are derived from measured peaks with headroom, never
   guessed, and every run re-measures. Where no user cgroup scope is available (CI containers),
   the harness says so once, in the log, and runs unenforced — it never pretends to enforce.
3. **The full ladder's verdict belongs to CI.** A workstation runs the subset that fits the
   machine it is on; a green `test-all` on a desktop is evidence, not authority. `ci-parity`
   already guarantees every workflow recipe is reachable from `test-all`; this ADR fixes the
   direction of trust between the two.

Supporting hygiene (not a rule, a consequence): `build/` carries a `CACHEDIR.TAG` — the
cross-tool "this is derived output" marker, the same thing cargo already writes into
`target/` and the portable form of the "never index this" marker reference systems put on
derived build output. Indexer- and
backup-specific exclusions beyond that standard are **developer setup, documented in
`docs/testing/README.md`** — the harness never silently mutates a developer's desktop
configuration.

Out of scope: choosing CI hardware, containerising the local build, and any per-machine
tuning of tools this repo does not own.

## Consequences

- **Positive**: a killed lane is distinguishable from a hung one, by evidence, in the run
  directory. A lane that regresses into consuming memory fails attributably instead of
  destabilising the host. The declared envelope is reviewable and diffable like every other
  profile property, and stays honest because every run records the peak it actually reached.
- **Positive**: the repo stops treating "the machine was busy" as an unexplained test result.
- **Negative / accepted churn**: the harness gains a re-exec step, so `qemu-test.sh` appears
  twice in a process tree; a developer reading `ps` sees the outer supervisor and the inner
  lane. Local runs on a loaded desktop can still be killed from outside — the envelope does
  not claim to prevent that, rule 3 does.
- **Measured (2026-09-11)**: a cold `make clean` + `just test-all` passed in one run with zero
  envelope events. Lanes peaked at 751-1,099 MiB; the cold compile outside the lanes peaked at
  1,768 MiB non-reclaimable (`anon`), 9,670 MiB including page cache. `MemoryHigh=4G` /
  `MemoryMax=8G` therefore carry every measured case with headroom; the recorded peaks remain
  the mechanism for tightening them.
