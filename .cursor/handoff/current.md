# Current Handoff: TASK-0016 remote packagefs RO proof close-out

**Date**: 2026-03-17  
**Status**: `TASK-0016` active; `os2vm` debugging + docs SSOT consolidation applied.  
**Contract baseline**: `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md` (`In progress`)

---

## What is stable now

- `tools/os2vm.sh` now provides:
  - phase gating (`RUN_PHASE=build|launch|discovery|session|remote|end`)
  - skip-build mode (`OS2VM_SKIP_BUILD=1`) with artifact validation
  - typed error classification (`OS2VM_E_*`) and typed exit mode (`OS2VM_EXIT_CODE_MODE=typed`)
  - structured run summaries (`os2vm-summary-<runId>.json/.txt`)
  - packet capture modes (`OS2VM_PCAP=off|on|auto`) and packet counter evidence
- Testing docs were consolidated:
  - new SSOT: `docs/testing/network-distributed-debugging.md`
  - `docs/testing/index.md` and `docs/testing/e2e-coverage-matrix.md` now link to SSOT
- Debugging-relevant CONTEXT headers synchronized in:
  - `source/services/dsoftbusd/src/os/session/cross_vm.rs`
  - `source/services/dsoftbusd/src/os/netstack/stream_io.rs`
  - `source/services/dsoftbusd/src/os/gateway/mod.rs`
  - `source/services/netstackd/src/main.rs`

## Current focus

- Close out `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md` using upgraded runtime evidence flow.
- Keep scope strict: remote packagefs RO behavior and proof completion only.

## Relevant contracts and linked work

- Active task:
  - `tasks/TASK-0016-dsoftbus-remote-packagefs-ro.md`
- Baseline contracts/docs:
  - `docs/rfcs/RFC-0028-dsoftbus-remote-packagefs-ro-v1.md`
  - `docs/adr/0005-dsoftbus-architecture.md`
  - `docs/distributed/dsoftbus-lite.md`
  - `docs/testing/index.md`
  - `docs/testing/network-distributed-debugging.md`
- Dependency tasks:
  - `tasks/TASK-0003-networking-virtio-smoltcp-dsoftbus-os.md`
  - `tasks/TASK-0003B-dsoftbus-noise-xk-os.md`
  - `tasks/TASK-0003C-dsoftbus-udp-discovery-os.md`
  - `tasks/TASK-0004-networking-dhcp-icmp-dsoftbus-dual-node.md`
  - `tasks/TASK-0005-networking-cross-vm-dsoftbus-remote-proxy.md`
  - `tasks/TASK-0015-dsoftbusd-refactor-v1-modular-os-daemon-structure.md`
- Follow-ons (do not absorb into this slice):
  - `tasks/TASK-0017-dsoftbus-remote-statefs-rw.md`
  - `tasks/TASK-0020-dsoftbus-streams-v2-mux-flow-control.md`
  - `tasks/TASK-0021-dsoftbus-quic-v1-host-first-os-scaffold.md`
  - `tasks/TASK-0022-dsoftbus-core-no_std-transport-refactor.md`

## Immediate next slice

1. Run `RUN_PHASE=session` and `RUN_PHASE=remote` loops with `OS2VM_EXIT_CODE_MODE=typed`.
2. Use `os2vm-summary-<runId>.json` as first-failure source, then correlate with UART/PCAP.
3. Complete RFC-0028 evidence update based on typed results and marker/packet correlation.
4. Confirm final full run (`RUN_PHASE=end`) with expected remote markers and `dsoftbusd: remote packagefs served`.

## Guardrails

- No fake success markers.
- No write opcodes for packagefs in this task.
- Reject non-`pkg:/` and non-`/packages/` paths deterministically.
- Keep wire/marker semantics stable unless task evidence updates contracts explicitly.
- Keep QEMU proofs sequential only.
- For cross-VM failures, classify via `os2vm` error code + summary before proposing fixes.
