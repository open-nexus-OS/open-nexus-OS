# Updates v1 — UI + CLI workflow (TASK-0140)

<!--
CONTEXT: How a developer drives the RFC-0089 OTA engine offline: the `nx
update` host CLI over built artifacts, and the Settings → Info → System
update page over the live services. The honest boundary of every surface
is stated where it applies.
OWNERS: @tools-team @runtime
STATUS: Functional
API_STABILITY: Unstable
ADR: docs/rfcs/RFC-0089-ota-v2-component-manifest-ab-boot-images-nxboot-bsb.md
-->

Both surfaces drive the REAL engine (`updated` v2 + bootctld) and hold no
update logic of their own. There is **no host↔guest transport**: the CLI
works on built artifacts (disk/data images), the page works on the live
services inside the OS.

## The honest boundary (read this first)

- **Commit happens via health quorum, never via a button.** A switch arms a
  bounded trial (tries=2); the commit fires only when the full declared
  quorum reports health after the reboot (RFC-0089 §13). Nothing on any
  surface can "force" a commit.
- **Mutation is policy-gated.** `updated` requires the policyd-granted
  `updates.manage` capability on the kernel-attributed sender for
  stage/switch/rollback (deny-by-default; reads stay open). The Settings
  page holds the route but NOT the grant — its action rows render the
  honest DENIED state. The CLI's switch/rollback verbs are offline
  preflights and say so (`applied=false`).

## `nx update` (host, offline)

```bash
nx update status  [--image build/nexus.img] [--key keys/dev-os-image]
nx update check   [--image build/nexus.img | --data data.img]
nx update stage <container.nxs> [--image build/nexus.img | --data data.img]
nx update switch  [--image build/nexus.img] [--tries 2]
nx update rollback [--image build/nexus.img]
```

- `status` decodes the BSB + both slot NXBDs from the disk through the same
  `bootfmt` codecs nxboot/bootctld link. Without a key, descriptors report
  `sig=unchecked`; verdicts are data, the exit class is the CLI contract.
- `check` enumerates `/updates/*.nxs` on the data volume and verifies every
  candidate through the REAL device engine against the baked anchor and the
  disk's anti-downgrade floor. A blank data volume is an honest empty feed.
- `stage` is the RFC-0089 §9 provisioning drop: verify FIRST (same engine,
  same floor gate — the stable reject vocabulary: `untrusted publisher |
  sig | digest | bounds | path | component kind unsupported | downgrade |
  io`), then write into `/updates/`. The device stages from there.
- `switch` / `rollback` are **preflights**: they report what the machine
  would do (`applied=false` in the JSON data) — the live transitions belong
  to `updated`/bootctld on the device.

After a QEMU OTA lane, `nx update status --image build/nexus.img` decodes
the state the LIVE machinery produced; the `ota-flip` harness gates on it.

## Settings → Info → System update (live)

The page reads `svc.updates.status` (updated forwards bootctld's status
plus the staged-build tail), `svc.updates.feed` and `svc.updates.check` —
active slot, pending trial, tries left, health commit, BSB projection,
rollback floor, staged build are all service truth, never derived
client-side. Actions ask the engine (`stage`/`switch`/`rollback`) and
render exactly what comes back: success, the machine's reject, or the
policy DENIED state.

Proof surfaces: the page logic is host-proven in
`tests/dsl_apps_conformance/tests/settings.rs` (service-truth rendering,
deny state, action→refresh chain); the live read surface is proven by
`SELFTEST: updates surface ok` (status/feed/check coherence against the
boot authority) in the QEMU ladder.
