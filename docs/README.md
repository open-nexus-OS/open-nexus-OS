---
schema: 1
title: "Documentation index"
role: index
subsystem: devx
verified: 2026-11-02
---

<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Documentation index

> **Purpose** — the one entry point: which folder answers which question, and every page with its role.
> **Authority** — `tasks/` is execution truth (status), `docs/rfcs/` are contracts, `docs/adr/` are
> decisions, the pages below explain how the code fits together. When they disagree: the ledger
> wins for status, the RFC for the contract, the code for what runs.
> **Read next** — [architecture/README.md](architecture/README.md) for the system map.

## Folders

| folder | answers |
|---|---|
| [architecture/](architecture/README.md) | the system map, the vision, the concurrency model |
| [kernel/](kernel/README.md) | NEURON: syscalls, scheduling, memory, capabilities, SMP |
| [boot-init/](boot-init/boot.md) | boot chain, nexus-init, execd, how a service lives and dies |
| [services/](services/README.md) | how a service is built, samgr, bundlemgrd |
| [security/](security/policyd.md) | policyd, capabilities, identity, signing, ABI filters, network policy, sandboxing |
| [storage/](storage/README.md) | packagefs, vfs, statefs, nxfs, VMOs, quotas |
| [packaging/](packaging/nxb.md) | `.nxb` bundles, artifact kinds, supply chain |
| [updates/](updates/README.md) | OTA, delta updates, system sets |
| [network/](network/dsoftbus.md) | DSoftBus, network authority, address matrix |
| [reliability/](reliability/observability.md) | logging, metrics, tracing, crash dumps, recovery, break-glass tokens |
| [graphics-ui/](graphics-ui/README.md) | windowd, gpud, the display chain, app surfaces, nexus-gfx |
| [testing/](testing/README.md) | test layers, QEMU profiles, markers, run logs, evidence |
| [standards/](standards/RUST_STANDARDS.md) | Rust, build, security and documentation standards |
| [rfcs/](rfcs/README.md) · [adr/](adr/README.md) | contracts and decisions (indexes: `nx ledger list --kind rfc`) |
| [dev/](dev/README.md) · [board/](board/) | developer guides (DSL, UI, SDK, `nx`) · the reference board's measurements |

## How a page stays true

Every page carries frontmatter (`role`, `subsystem`, `sources`, `contracts`, `verified`) and
`nx ledger lint` (in `just check`) rejects a dead path or link, a missing source, task status in
prose and a page over its word budget. `nx ledger stale` lists the pages whose `sources` changed in
a commit after their `verified` day; `just brief` counts them. After moving a page:
`nx ledger relink <old> <new>`. The rules: [DOCUMENTATION_STANDARDS.md](standards/DOCUMENTATION_STANDARDS.md).

## Every page

<!-- gen:docs-index -->
**architecture/**

| page | role | what |
|---|---|---|
| [README.md](architecture/README.md) | index | Architecture |
| [concurrency.md](architecture/concurrency.md) | guide | Concurrency model |
| [system-map.md](architecture/system-map.md) | guide | System map and boundaries |
| [vision.md](architecture/vision.md) | guide | Vision |

**boot-init/**

| page | role | what |
|---|---|---|
| [boot.md](boot-init/boot.md) | guide | Boot chain |
| [execd.md](boot-init/execd.md) | guide | execd |
| [lifecycle.md](boot-init/lifecycle.md) | guide | Process lifecycle and supervision |
| [nexus-init.md](boot-init/nexus-init.md) | guide | nexus-init |

**graphics-ui/**

| page | role | what |
|---|---|---|
| [README.md](graphics-ui/README.md) | guide | Graphics and UI stack |
| [animation-stack.md](graphics-ui/animation-stack.md) | guide | Animation stack |
| [display-chain.md](graphics-ui/display-chain.md) | guide | Display chain |
| [gpud-ring.md](graphics-ui/gpud-ring.md) | reference | gpud command ring and pipelined present |
| [nexus-gfx.md](graphics-ui/nexus-gfx.md) | reference | nexus-gfx crate |

**kernel/**

| page | role | what |
|---|---|---|
| [README.md](kernel/README.md) | guide | NEURON kernel |
| [smp.md](kernel/smp.md) | guide | SMP and IPIs |

**network/**

| page | role | what |
|---|---|---|
| [address-matrix.md](network/address-matrix.md) | reference | Network address matrix |
| [authority.md](network/authority.md) | guide | Networking authority |
| [dsoftbus.md](network/dsoftbus.md) | guide | DSoftBus (dsoftbusd) |

**packaging/**

| page | role | what |
|---|---|---|
| [artifact-kinds.md](packaging/artifact-kinds.md) | reference | Bundle types |
| [nxb.md](packaging/nxb.md) | reference | Bundles (.nxb) |
| [supply-chain.md](packaging/supply-chain.md) | reference | Bundle supply chain |

**reliability/**

| page | role | what |
|---|---|---|
| [crashdump-v2.md](reliability/crashdump-v2.md) | spec | Crash dumps (.nxcd) |
| [nxra.md](reliability/nxra.md) | guide | Recovery action tokens (.nxra) |
| [observability.md](reliability/observability.md) | guide | Observability |
| [recovery-operations.md](reliability/recovery-operations.md) | reference | Recovery operations |

**security/**

| page | role | what |
|---|---|---|
| [abi-filters.md](security/abi-filters.md) | reference | ABI argument filters (RFC-0091 profiles) |
| [capabilities.md](security/capabilities.md) | reference | Capability names |
| [identity.md](security/identity.md) | guide | Device identity and key custody |
| [network-policy.md](security/network-policy.md) | reference | Network ingress and egress policy |
| [policyd.md](security/policyd.md) | guide | policyd and the policy tree |
| [sandboxing.md](security/sandboxing.md) | reference | Sandboxing v1 |
| [signing.md](security/signing.md) | guide | Signing and trust anchors |

**services/**

| page | role | what |
|---|---|---|
| [README.md](services/README.md) | guide | Services guide |
| [bundlemgrd.md](services/bundlemgrd.md) | guide | bundlemgrd |
| [samgr.md](services/samgr.md) | guide | Service manager (samgrd + samgr) |

**standards/**

| page | role | what |
|---|---|---|
| [BUILD_STANDARDS.md](standards/BUILD_STANDARDS.md) | reference | Build standards |
| [DOCUMENTATION_STANDARDS.md](standards/DOCUMENTATION_STANDARDS.md) | reference | Documentation standards |
| [RUST_STANDARDS.md](standards/RUST_STANDARDS.md) | reference | Rust standards |
| [SECURITY_STANDARDS.md](standards/SECURITY_STANDARDS.md) | reference | Security standards |

**storage/**

| page | role | what |
|---|---|---|
| [README.md](storage/README.md) | guide | Storage |
| [nxfs.md](storage/nxfs.md) | guide | nxfs |
| [quotas.md](storage/quotas.md) | reference | State quotas |
| [statefs.md](storage/statefs.md) | spec | StateFS |
| [vmo.md](storage/vmo.md) | guide | VMOs |

**testing/**

| page | role | what |
|---|---|---|
| [README.md](testing/README.md) | guide | Testing guide |
| [e2e.md](testing/e2e.md) | reference | Host E2E suites |
| [evidence-bundle.md](testing/evidence-bundle.md) | spec | Evidence bundle |
| [layers.md](testing/layers.md) | reference | Test layers |
| [network-distributed-debugging.md](testing/network-distributed-debugging.md) | reference | Network and distributed debugging |
| [os-markers.md](testing/os-markers.md) | reference | OS markers |
| [proof-manifest.md](testing/proof-manifest.md) | reference | Proof manifest |
| [replay-and-bisect.md](testing/replay-and-bisect.md) | reference | Replay and bisect |
| [run-logs.md](testing/run-logs.md) | reference | Run logs and triage |

**updates/**

| page | role | what |
|---|---|---|
| [README.md](updates/README.md) | guide | Updates (OTA) |
| [delta.md](updates/delta.md) | guide | Delta updates |
| [system-set.md](updates/system-set.md) | reference | System set v1 (.nxs v1) |
<!-- /gen:docs-index -->
