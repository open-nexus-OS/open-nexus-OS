# RFC-0073: App files surface — `svc.files.*`, `nexus.permission.FILES`, the `filemanager` role, and the mime SSOT — contract seed

- Status: Draft (2026-07-15) — user decision 2026-07-15: stash ships as the privileged `filemanager` role (direct broad access); sandboxed apps get mediated pickers later (existing plan, TASK-0083/0084).
- Owners: @runtime
- Created: 2026-07-15
- Last Updated: 2026-07-15
- Links:
  - Tasks: `tasks/TASK-0291-vfs-readdir-svc-files-stash-real-listing.md` (P1), `tasks/TASK-0293-nxfsd-os-bringup-gpt-mount-data-keepblk.md` (P2 writes), `tasks/TASK-0294-mime-ssot-nexus-mime-icons-stash-filetype-icons.md` (mime/icons)
  - Related RFCs: `docs/rfcs/RFC-0072-vfs-v2-writable-providers-readdir-stable-errors.md` (the VFS surface underneath), `docs/rfcs/RFC-0071-nxfs-user-data-filesystem-contract.md` (the `/data` store), `docs/rfcs/RFC-0042-sandboxing-v1-vfs-namespaces-capfd-manifest-permissions-host-first-os-gated.md` (mediation machinery), `docs/rfcs/RFC-0065-ui-v6b-app-lifecycle-registry-notifications-navigation-contract.md` (manifest/launch authority)
  - Role model: `docs/dev/app-platform/privileged-roles.md` (`bundle_type = filemanager` (`FILES`) — planned row becomes real here)
  - Mime SSOT: `resources/mimetypes/mimetypes.toml` + `resources/mimetypes/README.md`
  - Track: `tasks/TRACK-STASH-USER-DATA-FS.md`

## Status at a Glance

- **Phase 1 (`svc.files.list/stat` + FILES permission + filemanager role + stash real listing)**: ✅ — `TASK-0291` (pack-ceiling deny test + `apphost: dsl svc files.list ok (n=3)` + stash screenshot evidence, 2026-07-15)
- **Phase 2 (`svc.files` write surface: mkdir/rename/remove/write via `/data`)**: ✅ — `TASK-0293` (mkdir boot-proven + cold-boot persistence, 2026-07-15)
- **Phase 3 (mime resolution + file-type icons in the DSL)**: ✅ — `TASK-0294` (`nexus-mime-icons` bake + `Image { source: "mime:…" }` + stash type icons; `stash: mime icons resolved (n=…)`, 2026-07-15)
- **Deferred (not this RFC)**: mediated pickers for sandboxed apps — `TASK-0083`/`TASK-0084`

Definition:

- "Complete" means the **contract** is defined and the **proof gates** are green (tests/markers). It does not mean "never changes again".

## Scope boundaries (anti-drift)

This RFC is a **design seed / contract**. Implementation planning and proofs live in tasks.

- **This RFC owns**:
  - The `svc.files.*` entries in `tools/nexus-idl/schemas/dsl_services.capnp` (names, args, results).
  - `nexus.permission.FILES` and its `SERVICE_ROUTES` row (`files` → `vfsd`).
  - The `filemanager` bundle type: manifest enum value, pack-time capability ceiling, launchability.
  - The mime resolution contract: extension → mime → icon-stem, SSOT file format, fallback chain.
  - The DSL image convention for file-type icons (`Image { source: "mime:<mime>" }`).
- **This RFC does NOT own**:
  - The vfs wire protocol / error codes (RFC-0072) or storage internals (RFC-0071/0018/0041).
  - Picker UX/protocol for sandboxed apps (explicitly deferred; new seed when TASK-0083/0084 activate).
  - The icon rasterizer (`nexus-svg`) or the bake mechanics (established by the app-icon pipeline).

### Relationship to tasks (single execution truth)

- `TASK-0291` proves Phase 1; `TASK-0293` Phase 2; `TASK-0294` Phase 3.

## Context

No `.nx` app can touch files: `dsl_services.capnp` has no files namespace, `SERVICE_ROUTES`
(`source/libs/nexus-sdk-routes/src/lib.rs`) has no files row, and no permission exists for it.
stash — the file manager, `userspace/apps/stash/` — renders six hard-coded rows and honestly says so.
`docs/dev/app-platform/privileged-roles.md` already reserves the answer: a `filemanager` bundle type
with a `FILES` permission, direct broad access for the role-holder, pickers for everyone else.
This RFC turns that row into a contract, routed through vfsd (which already owns per-app namespace
mediation via RFC-0042 — no new broker daemon).

## Goals

- A minimal, typed `svc.files` surface a DSL app can call: list, stat (Phase 1); read small,
  mkdir, rename, remove, write (Phase 2).
- Fail-closed permissioning: only apps granted `nexus.permission.FILES` get the route; the
  permission is ceiling-gated to privileged bundle types.
- `filemanager` as a real bundle type: pack-time enforcement (nxb-pack), registry launchability
  (bundlemgrd), stash as the first holder.
- One mime SSOT that maps extensions → mime types → icon stems, consumed by both the runtime
  (file-kind display) and the icon bake (TASK-0294) — never two parallel tables.

## Non-Goals

- Pickers / scoped grants for sandboxed apps (deferred to TASK-0083/0084; new contract seed then).
- Streaming large file content into DSL apps (bulk data stays in the VMO plane; the DSL surface
  returns bounded strings/lists in v1 — a viewer/opener contract is a future seed).
- Content sniffing (magic bytes). v1 resolves by extension only; the SSOT format reserves a
  `magic` field so sniffing can arrive without breaking the table.
- Mount management from apps. `svc.files` sees the namespace vfsd gives it; period.

## Constraints / invariants (hard requirements)

- **Determinism**: `svc.files.list` returns entries in the RFC-0072 canonical order; bounded page
  size; markers deterministic.
- **Fail-closed**: no FILES permission → route absent → `svc.files.*` returns the standard
  unavailable-service error; never a silent empty listing.
- **Bounded resources**: list replies bounded (≤ 64 entries/page, names ≤ 255 bytes) — DSL-side
  accumulation capped; no unbounded store growth from a hostile directory.
- **No fake success**: stash's demo data is deleted, not kept as fallback — an empty/failed listing
  renders an honest empty/error state.
- **One SSOT each**: service surface = `dsl_services.capnp`; routing = `SERVICE_ROUTES`; mime map =
  `resources/mimetypes/mimetypes.toml`. No parallel hand-maintained copies.
- **windowd untouched**: this is app/service plumbing; the compositor boundary (RFC-0067) is not crossed.

## Proposed design

### Contract / interface (normative)

**DSL surface** (`dsl_services.capnp`, sorted append per style contract):

```capnp
# Phase 1
(service = "files", method = "list", args = ["Str", "Int"], result = "List<FileEntry>"),
#   list(path, page) — page 0-based; FileEntry = { name: Str, kind: Str("file"|"dir"),
#   size: Int, mime: Str } (mime resolved via the SSOT; "" for dirs/unknown → fallback chain)
(service = "files", method = "stat", args = ["Str"], result = "FileEntry"),
# Phase 2
(service = "files", method = "mkdir",  args = ["Str"], result = "Bool"),
(service = "files", method = "readText", args = ["Str"], result = "Str"),      # bounded (≤ 8 KiB) — small text only
(service = "files", method = "remove", args = ["Str"], result = "Bool"),
(service = "files", method = "rename", args = ["Str", "Str"], result = "Bool"),
(service = "files", method = "writeText", args = ["Str", "Str"], result = "Bool"),  # bounded
```

Errors surface to the DSL as the standard `svc` error result carrying the RFC-0072 code name
(e.g. `EACCES`, `ENOTFOUND`) — apps can branch on them.

**Route row** (`nexus-sdk-routes`):

```rust
ServiceRoute { svc: "files", route: "vfsd", permission: "nexus.permission.FILES", child_slot: 16 }
```

Direct to vfsd — per-app mediation is vfsd's namespace layer (RFC-0042). A separate `filesd`
broker was considered and rejected (Alternatives). Slot 16 = next free (14 is the app-host events
slot; routes skip it by contract).

**`filemanager` bundle type**:

- `manifest.capnp` `BundleType` gains `filemanager` (append, no renumbering).
- nxb-pack: `bundle_type = "filemanager"` maps to it; the capability ceiling table allows
  `nexus.permission.FILES` **only** for `filemanager` (and `settings`-tier system bundles if ever
  needed) — a plain `app` declaring FILES fails at pack time (deterministic error).
- bundlemgrd: `filemanager` is launchable (like `app`/`settings`).
- Namespace policy (normative): a `filemanager` gets the full user-visible tree
  (`/data`, `/packages` RO); plain apps (future pickers) get scoped grants only. Enforced in the
  launch authority's namespace provisioning, not in the app.
- stash's manifest flips to `bundle_type = "filemanager"`, `caps += ["nexus.permission.FILES"]`.

**Mime SSOT** (`resources/mimetypes/mimetypes.toml`):

```toml
# extension → mime → icon stem (stems are files in resources/mimetypes/<stem>.svg)
[types.jpg]  mime = "image/jpeg"   icon = "image-jpeg"
[types.jpeg] mime = "image/jpeg"   icon = "image-jpeg"
[types.rs]   mime = "text/x-rust"  icon = "text-x-rust"
# ...
[fallbacks]
image = "image-x-generic"   # mime class → generic stem
audio = "audio-x-generic"
video = "video-x-generic"
text  = "text-plain"
directory = "inode-directory"
unknown = "application-octet-stream"
```

Resolution chain (normative): extension match → exact-mime stem → mime-class generic →
`application-octet-stream`. Directories always `inode-directory`. The same file feeds
(a) the files service's `mime` field and (b) the `nexus-mime-icons` bake (TASK-0294) —
one table, two consumers.

**DSL icon convention** (Phase 3, implemented): `Image { source: "mime:<token>", size: <px> }` —
the runtime resolves via the baked mime-icon table exactly like app icons resolve by app id
(`userspace/dsl/runtime/src/registry.rs` Image primitive). The token is, in priority order: an
already-resolved icon stem (the app-host's fast path — it emits `"mime:<stem>"` per listing entry),
a full mime type (contains `/`), or a bare file extension. All three run the same normative chain
and always land on a real stem; unknown input renders `application-octet-stream`, never a blank.
The resolution SSOT lives once in `nexus-mime-icons` (built from `mimetypes.toml`) and is shared by
both the app-host (`entry_icon_stem`) and the DSL primitive (`sprite_for_source`).

### Phases / milestones (contract-level)

- **Phase 1** (`TASK-0291`): list/stat + permission + role + stash lists `/packages` for real.
- **Phase 2** (`TASK-0293`): write surface over `/data` (nxfs mounted RW); stash mkdir/rename/delete.
- **Phase 3** (`TASK-0294`): mime SSOT wired into the service + `nexus-mime-icons` bake + stash rows
  show real per-type icons.

## Security considerations

- **Threat model**: over-broad file access from ordinary apps (→ ceiling: FILES only packs for
  `filemanager`); confused deputy via crafted paths (→ vfsd canonicalization + namespace, RFC-0042);
  hostile directory contents blowing up the UI (→ bounded pages, name caps, no auto-open).
- **Mitigations**: pack-time ceiling (deny at build), launch-time fail-closed route provisioning,
  vfsd-side namespace mediation, RFC-0072 audit on deny.
- **DON'T DO**: never grant FILES to `bundle_type = app` "temporarily"; never bypass vfsd with a
  direct storage route for an app; never resolve mime by executing/sniffing content in v1.
- **Open risks**: a granted filemanager sees everything user-visible by design (that IS the role);
  revocation/UX consent flows arrive with the picker seed.

## Failure model (normative)

- Route absent (no permission) → standard unavailable-service DSL error; deterministic.
- vfsd errors pass through as their RFC-0072 code names; stash renders error/empty states honestly.
- Oversize `readText`/`writeText` → `E2BIG` (bounded at 8 KiB inline; larger content is not a DSL
  surface in v1).
- Unknown extension → `mime = ""` + fallback icon; never a guess presented as fact.

## Proof / validation strategy (required)

### Proof (Host)

```bash
cd /home/jenning/open-nexus-OS && cargo test -p nexus-sdk-routes   # table consistency incl. new row
cd /home/jenning/open-nexus-OS && cargo test -p nxb-pack           # ceiling: FILES rejected for bundle_type=app
cd /home/jenning/open-nexus-OS && cargo test -p vfsd               # RFC-0072 surface
```

### Proof (OS/QEMU)

```bash
cd /home/jenning/open-nexus-OS && RUN_UNTIL_MARKER=1 RUN_TIMEOUT=190s just test-os
```

Plus the visible-boot evidence contract: stash launched by click, screenshot shows real entries.

### Deterministic markers (if applicable)

- `app-host: svc.files routed (slot=16)` (Phase 1)
- `stash: listing real (n=<count>)` (Phase 1)
- `SELFTEST: files denied without cap ok` (Phase 1 negative)
- `stash: write ok` (Phase 2)
- `stash: mime icons resolved (n=<count>)` (Phase 3)

## Alternatives considered

- **A `filesd` broker daemon between apps and vfsd** — rejected: vfsd already owns per-app
  namespace mediation + CapFd (RFC-0042); a broker duplicates that seam and adds a hop. Revisit
  only if picker grants need stateful UX brokering (that seed may introduce one for pickers alone).
- **Granting FILES as an ordinary permission any app may declare** — rejected: violates the
  privileged-roles ceiling model; pickers are the sandboxed path.
- **Mime table in code (match statement)** — rejected: two consumers (service + icon bake) would
  drift; TOML resource is the SSOT and diff-reviewable.
- **Icon-per-extension keying** — rejected: extensions alias (jpg/jpeg); mime is the stable key,
  extensions map into it.

## Open questions

- `/data` top-level layout (shared user tree vs per-app homes + shared) — decide in TASK-0293
  when the RW mount lands; namespace policy hook is already normative here.
- Whether `files.list` should return mtime for sorting (couples with the RFC-0072 open question).

## RFC Quality Guidelines (for authors)

When writing this RFC, ensure:

- Scope boundaries are explicit; cross-RFC ownership is linked.
- Determinism + bounded resources are specified in Constraints section.
- Security invariants are stated (threat model, mitigations, DON'T DO).
- Proof strategy is concrete (not "we will test this later").
- If claiming stability: define ABI/on-wire format + versioning strategy.
- Stubs (if any) are explicitly labeled and non-authoritative.

---

## Implementation Checklist

**This section tracks implementation progress. Update as phases complete.**

- [x] **Phase 1**: `svc.files.list/stat` + FILES + filemanager role + stash real listing — proof: `apphost: dsl svc files.list ok (n=3)` + `execd: app route granted svc=files` + visible-boot screenshots; pack-time deny `test_reject_files_cap_for_plain_app_bundle_type` (TASK-0291, 2026-07-15; runtime deny selftest deferred, see task)
- [ ] **Phase 2**: write surface over `/data` — proof: `stash: write ok` (TASK-0293)
- [ ] **Phase 3**: mime SSOT + icon bake + stash type icons — proof: `stash: mime icons resolved (n=<count>)` (TASK-0294)
- [ ] Task(s) linked with stop conditions + proof commands.
- [ ] QEMU markers appear in `scripts/qemu-test.sh` and pass.
- [ ] Security-relevant negative tests exist (`test_reject_*`: FILES-for-app pack rejection, route-absent deny).
