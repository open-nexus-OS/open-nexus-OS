---
title: TASK-0139 Network Basics v1b (offline): Settings Network page + nx-net CLI + policy gate + OS selftests/postflight + docs
status: Draft
owner: @ui
created: 2025-12-25
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Offline net daemons: tasks/TASK-0138-network-basics-v1a-offline-controlplane-daemons.md
  - SystemUI→DSL Settings baseline: tasks/TASK-0121-systemui-dsl-migration-phase2a-settings-notifs-host.md
  - Policy gates (network.configure): tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md
  - Testing contract: scripts/qemu-test.sh
---

## Context

Once the offline networking control-plane daemons exist (`TASK-0138`), we need user-visible controls and
developer tooling:

- Settings → Network page (DSL),
- a small CLI (`nx net`) for deterministic automation,
- OS selftests and a postflight that proves the QEMU run without relying on external networking.

## Goal

Deliver:

1. Settings → Network (DSL) page:
   - status card: mode, iface, address/gateway/DNS, lease seconds (if present)
   - controls:
     - mode selector (Offline / Airplane / Sim DHCP / Static)
     - static fields (IP/mask/gw/dns)
     - hosts editor (writes to `state:/net/hosts.json` if `/state` exists; otherwise uses in-memory override with explicit “non-persistent” label)
     - “Sync Time Now” (calls timesyncd)
   - markers:
     - `settings:network open`
     - `settings:network mode=<...>`
     - `settings:network static set`
     - `settings:network dns host added`
2. CLI `nx-net`:
   - `nx net status`
   - `nx net mode get|set ...`
   - `nx net static set ...`
   - `nx net dns resolve/preload/flush/stats`
   - `nx net time sync`
   - stable output lines designed for deterministic parsing
3. Policy integration (optional but preferred):
   - configuration operations require `policyd.require(network.configure)` (system-only by default)
   - if policyd is not present yet in the OS build, the task must:
     - either gate config operations to “system subject” only, or
     - emit an explicit `stub` marker and skip enforcement (must be honest)
4. OS selftests (bounded):
   - wait for `netcfgd/dhcpcd/dnsd/timesyncd: ready`
   - switch to simDhcp, resolve `example.local`, add a hosts override, run syncNow
   - print:
     - `SELFTEST: net v1 dhcp ok`
     - `SELFTEST: net v1 dns ok`
     - `SELFTEST: net v1 hosts ok`
     - `SELFTEST: net v1 time ok`
5. Postflight + docs:
   - `tools/postflight-net-v1.sh` delegates to host tests + bounded QEMU run
   - docs under `docs/network/` plus testing marker list

## Non-Goals

- Kernel changes.
- Enabling webviewd http(s) navigation (stays blocked).
- Real DHCP/DNS/NTP networking (that’s `TASK-0004`+future).

## Constraints / invariants (hard requirements)

- Offline-only: no external network sockets and no dependency on QEMU net backends.
- Deterministic UI tests and deterministic CLI output.
- No `unwrap/expect`; no blanket `allow(dead_code)`.

## Stop conditions (Definition of Done)

### Proof (Host) — required

- `network_basics_v1_host` stays green
- new host tests for UI/CLI (goldens) if added

### Proof (OS/QEMU) — required

UART includes:

- `netcfgd: ready`
- `dhcpcd: ready`
- `dnsd: ready`
- `timesyncd: ready`
- `SELFTEST: net v1 dhcp ok`
- `SELFTEST: net v1 dns ok`
- `SELFTEST: net v1 hosts ok`
- `SELFTEST: net v1 time ok`

## Touched paths (allowlist)

- `userspace/systemui/dsl/pages/settings/Network.nx` (new)
- `userspace/systemui/dsl_bridge/` (extend for netcfg/dns/time)
- `tools/nx-net/` (new)
- `source/apps/selftest-client/`
- `tools/postflight-net-v1.sh`
- `docs/network/`

## Plan (small PRs)

1. DSL Network page + bridge adapters + markers (host-first)
2. nx-net CLI + stable output
3. selftests + postflight + docs

