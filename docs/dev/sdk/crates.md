<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# The App SDK crate surface

Apps link system libraries DIRECTLY — that is what they exist for (audio,
video, gfx, text, layout processing). But only a **curated** set is
SDK-public; the SSOT is [`crates.toml`](crates.toml) next to this document
(machine-readable — the companion dep-gate and `tests/sdk_surface` consume
it). Decision record: TASK-0081, decision C0 (2026-07-07).

## The rule

- **Processing = library.** Decoding, measuring, laying out, rasterizing,
  querying — link the SDK crate, run in YOUR process, no capability needed.
- **Actuation = service + capability.** Audio *output*, camera, GPU
  *present*, files, network — always `svc.*` behind a manifest-declared
  permission (`nexus.permission.*`), fail-closed in abilitymgr/policyd.
- OS-internal crates (raw `nexus-abi` syscalls, `nexus-ipc`, kernel-adjacent
  libs) are **never** SDK-public: they are the trust boundary, and their API
  stability is not promised to apps.

## Consumers

- `native/` companion crates (TASK-0081 decision C1): may depend on SDK
  crates + their own vendored code, NOTHING else from the workspace — the
  dep-gate that enforces this rides with the `nx dsl add native` tooling.
- AOT apps (`payload_kind = "elf"`, TASK-0079): same list; they additionally
  link `nexus-dsl-runtime` as the interpreter host.
- Versioning: the existing manifest `min_sdk` field gates the set an app was
  built against.

## Changing the list

Adding a crate is an API-stability commitment: it needs an owner, a
`STATUS: Functional` header, and a review that it contains no actuation
paths. Removing one is a breaking SDK change. Both go through this file +
`crates.toml` in the same commit — the `tests/sdk_surface` host test fails
on drift between the two and on entries whose `path` does not exist.
