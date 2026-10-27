<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# ADR-0084: One permission authority — policyd holds every app grant, the bundle manager publishes them from the verified volume, the launchers ask and never read a manifest

- Status: Accepted (2026-10-27; headless QEMU lane of TASK-0333 6d-3 — the board cycle follows
  with the layer's commit)
- Date: 2026-10-27
- Links:
  - Tasks: `tasks/TASK-0333-memory-the-efficient-one-and-the-app-lifecycle-layer-6.md` (6d-3)
  - RFCs: `docs/rfcs/RFC-0111-process-lifecycle-freeze-thaw-kill-the-parent-orders-the-process.md`
    (§6 — this decision fixes the authority; §4 the one grant with a lifecycle effect,
    `nexus.permission.BACKGROUND`), `docs/rfcs/RFC-0065-…` (abilitymgr is the launch
    authority), `docs/rfcs/RFC-0089-…` (the verified system volume the grants are read from)
  - Related ADRs: `docs/adr/0014-policy-architecture.md` (policyd decides, deny-by-default),
    `docs/adr/0083-…` (the lifecycle contract the background grant exempts an app from),
    `docs/adr/0036-…` (lifecycle vs process vs registry: three services, three truths)

## Context

Until 6d-3 an app's permissions lived in THREE places, none of them an authority: abilitymgr
and execd each compiled a table of every manifest's `caps` into their own binary at build time
(`APP_MANIFEST_CAPS`, two build scripts reading the same files), and bundlemgrd carried the
manifest sidecar on the volume without reading the caps at all. A permission was "granted"
because a build happened to see the manifest — an installed bundle whose manifest changed, a
bundle installed after the build, or a build of one service against another tree were all
grants nobody had decided. abilitymgr validated the names at boot against its own list and
printed `caps ok`; execd provisioned a child's service routes from its own copy. The manifest
`background = [...]` of RFC-0111 §4 had to go somewhere, and a fourth table was the wrong
answer.

The systems this OS is measured against hold permissions in one place: an installed app's
grants are a record the package installer writes and the permission service answers, and the
launcher asks that service at every start. Android's PackageManager and OpenHarmony's
AccessTokenManager are that one place; nothing reads a manifest at launch.

## Decision

- **policyd is the one permission authority for apps.** It holds, per installed app, the set
  of grants (`nexus.permission.*` and the app-owned `app.<bundle>.<CAP>`), ONE bounded table
  (32 apps × 16 grants × 48 bytes), replaced per app on every publish. Two new verbs on the
  policyd wire (v2 nonce frames, `nexus-wire`): `OP_APP_GRANTS_PUT` (10) and
  `OP_APP_GRANTS_GET` (11). Who may publish and who may read is the policy table's own
  business: `app.grants.publish` (bundlemgrd) and `app.grants.read` (abilitymgr, execd, the
  proof harness), checked against the KERNEL sender id — never a name in the frame.
- **The bundle manager publishes, once per boot, from the verified volume.** `nx app compile`
  writes the manifest's `caps` and `background` into the `meta/app.properties` sidecar the
  volume carries (digest-bound like every file of it); bundlemgrd parses them with the label
  and publishes every app before it answers the first volume request after its policyd leg is
  live. A non-empty `background = [...]` becomes the one grant `nexus.permission.BACKGROUND`;
  a manifest may not name that grant in `caps` directly (`nxb-pack` refuses it, and bounds the
  mode list to 8 names of 1..=32 `[a-z0-9-]`).
- **Unknown names reject the app, not the name.** The authority recognizes the platform
  permissions and the app-owned namespace by shape; a publish naming anything else is refused
  whole and removes any earlier entry for that app (`policyd: app grants reject (app= cap=)`) —
  an app's grants are either the manifest's set or nothing, never yesterday's set.
- **The launchers ASK, fail-closed.** abilitymgr asks at every launch over its declared
  policyd leg (`wire::GrantSource`): no answer — DENY, a transport failure, a slot init never
  pinned — is a refused launch with its own marker (`abilitymgr: launch denied (grants app=`),
  and the grants it records on the instance are the authority's answer, nothing else. execd
  asks at every spawn: the SDK service routes a child gets are exactly the routable grants of
  the answer; no answer, no routes (`execd: FAIL app grants (app= …)`). Both build-time
  tables, both build-script readers, abilitymgr's `KNOWN_PERMISSIONS` and its boot-time
  `caps ok` self-check are deleted. abilitymgr keeps the export table (which app exports which
  ability) — that is the mediation's knowledge, not a grant.
- **The background grant has exactly one effect in 6d**: an instance holding it is never frozen
  for being hidden (ADR-0083's grace never arms for it); every other instance is
  (deny-by-default). No shipped app declares it; the contract and its refusal are proven on the
  host (`test_reject_background_undeclared`) and the live set is read back on the headless lane
  (`SELFTEST: app grants ok (app=calculator n= background=0)`); the allow side's runtime
  effect is not claimed board-proven until an app uses it.

## Consequences

- One `cargo build` no longer decides what an app may do; the installed volume does. An app
  installed, upgraded or removed without rebuilding the services gets its grants from the same
  place as every other.
- Every launch costs one round trip abilitymgr → policyd and one execd → policyd (two short
  frames each, on legs init pins at boot); policyd's reads print nothing (the hot path), its
  publishes print one line per app.
- A manifest typo is visible once, at publish, by name, and the app does not launch — no
  silent launch with a smaller set.
- The app-owned namespace is accepted by SHAPE at publish (`app.<bundle>.<CAP>`); whether an
  exporter exists is the mediation's check at resolve time (`UnknownAbility`), as before. A
  consumer granted a permission nobody exports holds a grant that resolves nothing.
- Three declared legs were added to the topology (abilitymgr → policyd, bundlemgrd → policyd
  late-grant; execd already had one for the crash gate); `policies/base.toml` grew two
  capabilities. The route-table SSOT (`REQUIRED_ROUTES`) and the init topology tests carry
  them.

## Alternatives considered

- **Keep abilitymgr's table and add `background` to it** (the ledger's original 6d-3 row): a
  fourth copy of the manifests, still decided by a build, and the service launcher would still
  provision routes from its own copy — two authorities drift the day one is rebuilt alone.
- **bundlemgrd answers grants itself** (it has the sidecars): the registry becomes a policy
  authority, and a volume attach failure makes every launch a registry question; policyd
  already is the deny-by-default authority every sensitive op goes through (ADR-0014).
- **Publish at install only, not per boot**: the table would have to persist across boots
  (statefs) and be trusted after a volume swap; publishing from the VERIFIED volume at every
  boot keeps the grants exactly as trustworthy as the volume's digest chain, with no second
  store to reconcile.
