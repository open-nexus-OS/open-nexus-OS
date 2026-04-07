<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Capabilities

Open Nexus OS uses a capability-based model rather than ambient authority. This page collects the capability names that
already exist in the repo, plus the capability families that are clearly planned in active tracks.

This page is a **catalog and naming guide**, not the single enforcement source of truth.

## Where the truth lives

Use these sources together:

- current baseline allowlist: `recipes/policy/base.toml`
- policy authority and evaluation model: `docs/architecture/11-policyd-and-policy-flow.md`
- signing/install/policy flow: `docs/security/signing-and-policy.md`
- capability-driven track contracts: for example `tasks/TRACK-NEXUSNET-SDK.md` and `tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md`

## Current baseline capabilities (enforced today)

These names appear in the current baseline policy file under `recipes/policy/base.toml`.

### Core routing and process

- `ipc.core`
- `proc.spawn`
- `policy.delegate`
- `time.read`

### Files and package access

- `fs.read`
- `fs.verify`

### Crypto and entropy

- `crypto.sign`
- `crypto.verify`
- `rng.entropy`

### Device identity and key custody

- `device.keygen`
- `device.pubkey.read`
- `device.key.reload`

### StateFS and persistence

- `statefs.read`
- `statefs.write`
- `statefs.boot`
- `statefs.keystore`

### Device MMIO

- `device.mmio.blk`
- `device.mmio.net`
- `device.mmio.rng`

## Additional capabilities already appearing in recipes

These names appear in recipe manifests today and should be treated as real capability strings already in circulation.

### UI / shell / graphics

- `ability.start`
- `window.launch`
- `window.manage`
- `graphics.compose`

### Distributed / service-facing

- `distributed.bus`

## Planned capability families with explicit track anchors

The following names are not all globally enforced today, but they are already spelled out in active tracks and should be
kept consistent as the capability catalog grows.

### Networking and cloud

From `tasks/TRACK-NEXUSNET-SDK.md`:

- `network.tcp.connect`
- `network.tcp.listen`
- `network.udp.send`
- `network.udp.bind`
- `network.http.request`
- `cloud.sync`
- `cloud.graphql.query`
- `cloud.graphql.mutate`
- `cloud.oauth2.start`
- `cloud.oauth2.finish`
- `cloud.oauth2.token.refresh`
- `account.manage`
- `account.use`

### DSoftBus / distributed app model

From `tasks/TRACK-NEXUSNET-SDK.md`:

- `dsoftbus.discover`
- `dsoftbus.session.open`
- `dsoftbus.stream.open`
- `dsoftbus.rpc.call`
- `dsoftbus.share.send`
- `dsoftbus.share.receive`

### App-facing policy matrix

From `tasks/TASK-0136-policy-v1-capability-matrix-foreground-adapters-audit.md`:

- `content.read`
- `content.write.state`
- `clipboard.read`
- `clipboard.write`
- `camera`
- `microphone`
- `screen.capture`
- `location.coarse`
- `location.precise`
- `audio.output`
- `notifications.post`
- `intents.send`
- `intents.receive`
- `webview.net`
- `storage.manage`
- `sms.send`
- `sms.receive`
- `mms.send`
- `mms.receive`

### Store and package-management examples

From store/package tracks:

- `store.feed.read`
- `store.install`
- `store.remove`
- `store.rate.write`

### Media, search, backup, battery, and similar subsystem examples

Capability strings already proposed in task contracts include:

- `media.remote.publish`
- `media.remote.control`
- `media.session.publish`
- `media.session.control`
- `media.volume.set`
- `media.mute.set`
- `search.query`
- `search.index.write`
- `search.debug`
- `backup.create`
- `backup.restore`
- `backup.read`
- `backup.delete`
- `battery.status.read`
- `battery.calibrate`
- `battery.inject`
- `power.lowpower.enter`
- `power.shutdown.request`

### Delegation and app-surface examples

Track-level examples that look like capability or action-style names and should stay consistent with capability naming
work:

- `chat.compose`
- `contacts.pick`
- `social.compose`
- `maps.pick_location`
- `track.inline`
- `confirm.inline`
- `open.inline`

Some of these may ultimately remain system-delegation action IDs rather than raw policy capabilities. Document the
difference explicitly when the contract is finalized.

## Naming guidance

Capability names should stay:

- short,
- stable,
- lowercase,
- and grouped by domain.

Recommended pattern:

- `<domain>.<verb>`
- `<domain>.<resource>.<verb>` when one segment is not enough

Examples:

- `crypto.sign`
- `network.http.request`
- `device.mmio.net`
- `cloud.oauth2.token.refresh`

Avoid:

- UI-marketing names as capability strings,
- duplicated names that differ only by punctuation or case,
- or names that describe a screen instead of an authority.

## Scope and interpretation

Capability strings should describe **authority**, not aesthetics or implementation details.

Good examples:

- "may sign with this service"
- "may request HTTP"
- "may publish a media session"

Less useful examples:

- screen names,
- generic product labels,
- or names that smuggle multiple unrelated powers into one string.

## Notes for future developer-tooling work

The planned developer workstation model will likely need additional capability families for:

- runtime installation and use,
- tool installation and execution,
- local service lifecycle,
- package-manager network profiles,
- and shell or automation posture.

When those names are introduced, extend this page and clearly label whether they are:

- current baseline,
- current recipe usage,
- or planned catalog entries.

## Related docs

- `docs/security/signing-and-policy.md`
- `docs/security/shell-scripts-and-automation.md`
- `docs/architecture/11-policyd-and-policy-flow.md`
- `docs/packaging/artifact-kinds.md`
