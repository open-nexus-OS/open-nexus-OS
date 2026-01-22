---
title: TRACK Media Apps (Photos + Music + Videos): library-first, streaming-ready, cloud-integrated (first-party reference apps)
status: Living
owner: @media @ui
created: 2026-01-19
links:
  - Vision: docs/agents/VISION.md
  - Playbook: docs/agents/PLAYBOOK.md
  - Zero-Copy App Platform (foundation): tasks/TRACK-ZEROCOPY-APP-PLATFORM.md
  - NexusMedia SDK (decoders/playback): tasks/TRACK-NEXUSMEDIA-SDK.md
  - NexusNet SDK (cloud + streaming APIs): tasks/TRACK-NEXUSNET-SDK.md
  - Podcasts app (offline downloads + queue): tasks/TRACK-PODCASTS-APP.md
  - Service architecture (hybrid control/data plane): docs/adr/0017-service-architecture.md
  - Media sessions + SystemUI controls umbrella: tasks/TASK-0101-ui-v16c-media-sessions-systemui-controls.md
  - Media UX v1 (host core): tasks/TASK-0155-media-ux-v1a-host-mediasessd-focus-nowplaying-artcache.md
  - Media UX v1 (OS/QEMU mini-player + lockscreen): tasks/TASK-0156-media-ux-v1b-os-miniplayer-lockscreen-sample-cli-selftests.md
  - Media UX v2 (OS/QEMU session switcher + notif actions): tasks/TASK-0185-media-ux-v2b-os-miniplayer-session-switch-notifs-selftests.md
  - Media UX v2.1 (OS/QEMU focus/ducking + per-app volume + mini-player): tasks/TASK-0218-media-v2_1b-os-focus-ducking-miniplayer-nx-media.md
  - DSoftBus media cast picker + remote controls: tasks/TASK-0220-dsoftbus-v1_2b-os-media-cast-picker-nx-media-remote.md
  - Image Viewer baseline: tasks/TASK-0090-ui-v13d-image-viewer-export-print.md
  - Media decoders: tasks/TASK-0099-ui-v16a-media-decoders.md
  - Music/Video player apps: tasks/TASK-0102-ui-v16d-music-video-apps-os-proofs.md
  - Camera + Gallery (capture only): tasks/TASK-0106-ui-v17d-camera-app-gallery-settings-os-proofs.md
  - MIME/content foundations: tasks/TASK-0081-ui-v11a-mime-registry-content-providers.md
---

## Goal (track-level)

Deliver a first-party Media Apps suite that proves the ecosystem story:

- **Photos** (like Apple Photos): library-first, timeline/albums/faces, smart search, iCloud-style sync,
- **Music** (like Apple Music): **Listen Now** start page + local library + streaming (Tidal/SoundCloud/Netease) + search,
- **TV** (like Apple TV): **Watch Now** hub + curated local library + provider “channels” + device casting/streaming,
- **double strategy**: click a file → lightweight viewer/player; browse library → full-featured app with indexing/metadata/cloud,
- **always-on sync** (optional cloud backends; local-first by default),
- and a shared core so that "it feels like someone else built it" (consistent UX + primitives).

The suite is a reference implementation of `tasks/TRACK-ZEROCOPY-APP-PLATFORM.md` and `tasks/TRACK-NEXUSMEDIA-SDK.md`.

## Product stance (what makes this "better than iTunes/Photos/VLC")

- **Library-first indexing where it helps**:
  - Photos: automatic scan of `state:/pictures/` (incl. captures) with EXIF metadata extraction.
  - Music: automatic scan of `state:/music/` with tag extraction (ID3/Vorbis/etc.).
  - TV: **no Kodi-style automatic “scan everything”** by default. TV library is **curated** (user-added), so camera/screen recordings don’t flood it.
- **Streaming-ready**: cloud APIs (Tidal/SoundCloud/Netease for music; Netflix/YouTube-style for video) as first-class citizens.
- **Casting-native**: AirPlay/Chromecast-style remote playback via DSoftBus (no vendor lock-in).
- **Privacy-first**: all cloud/streaming features are capability-gated and auditable; no ambient "phone home".
- **Zero-copy data plane**: large media files use VMO/filebuffer semantics (fast load, low memory overhead).
- **Deterministic proofs**: host-first tests for indexing/metadata/playback; QEMU markers for OS integration.

## Shared core (required)

The suite shares a common foundation (library and/or service boundaries):

- **Media indexer service (`mediad`)**: scans local storage (Photos/Music), extracts metadata, provides a query API (by artist/album/date/etc.).
- **Streaming connectors**: pluggable **providers** for Music and TV via NexusNet SDK (OAuth2/account model; typed stubs preferred).
- **Provider “store” surfaces**:
  - Music: provider login/enablement inside the app (Apple Music style).
  - TV: “Channels” store inside the app (install/enable providers like Rakuten, ZDF, Mubi, Red Bull TV).
- **Global media UX surfaces (system-wide)**:
  - `mediasessd` (now-playing + focus + control routing),
  - SystemUI mini-player (tray/control center), lockscreen tile, and notifications (gated),
  - `nx media` / `nx-media` tools for deterministic control and selftests.
  These are shared across **all** media apps (players, Photos, Music, TV) and are tracked in `TASK-0101`, `TASK-0155`, `TASK-0156`, `TASK-0185`, `TASK-0218`, and `TASK-0220`.
- **Casting protocol**: DSoftBus-based remote playback with media session sync (play/pause/seek state).
- **Sync engine**: optional iCloud-style sync for libraries/playlists/watch history (via NexusNet cloud.sync).

Note: This track does not mandate a single "monolithic app"; it mandates shared contracts and primitives.

## App architecture: double strategy (6 apps total)

### 1. Image Viewer (single-file viewer)

**Scope**: open a single image file (PNG/JPEG/SVG/GIF/APNG) via picker or "Open With".

**Features**:

- zoom/pan/rotate/flip
- export as PNG
- clipboard copy
- print integration

**Status**: ✅ **Already planned** in `tasks/TASK-0090-ui-v13d-image-viewer-export-print.md`

---

### 2. Photos (library app, like Apple Photos)

**Scope**: index ALL photos/images on the device + optional cloud sync.

**Features**:

- **Library indexing**:
  - scan `state:/pictures/` recursively (not just camera captures)
  - extract EXIF metadata (date/location/camera model)
  - generate thumbnails via `thumbd`
- **Views**:
  - timeline (by date)
  - albums (user-created + smart albums)
  - faces (optional ML-based face grouping)
  - map view (if location data present)
- **Search**:
  - by date/location/camera/tags
  - smart search (e.g., "photos from last summer")
- **Cloud sync** (optional):
  - iCloud-style sync via `svc.cloud.sync.*`
  - capability-gated: `cloud.sync.photos`
  - audit logs for sync events
- **Integration**:
  - opens single image in Image Viewer (double-click)
  - share/delete/rename/move to album
  - export selections

**Status**: ⚠️ **Partially planned** in `tasks/TASK-0106-ui-v17d-camera-app-gallery-settings-os-proofs.md` (but only for camera captures, not full library)

**Action needed**: Extend TASK-0106 or create new task for full Photos library app.

---

### 3. Music Player (single-file/playlist player)

**Scope**: play a single audio file or a playlist (WAV/OGG/MP3/FLAC).

**Features**:

- open via picker or "Open With"
- play/pause/seek/volume
- playlist view (minimal)
- media session integration (SystemUI controls)
- notification actions (play/pause/next/prev)

**Status**: ✅ **Already planned** in `tasks/TASK-0102-ui-v16d-music-video-apps-os-proofs.md`

---

### 4. Music (library app, like Apple Music)

**Scope**: Apple Music-style app with **Listen Now**, provider streaming, and a local library.

**Features**:

- **Tabs / IA**:
  - **Listen Now**: recommendations, “recently played”, “up next/queue”, mixes (provider-sourced initially; hub-composed later)
  - **Browse**: charts, genres, editorial sections (provider-sourced)
  - **Library**: local music and user playlists
  - **Search**: across local library and enabled providers
- **Library indexing**:
  - scan `state:/music/` recursively
  - extract ID3/Vorbis tags (artist/album/genre/year/cover art)
  - generate waveform thumbnails (optional)
- **Views (Library)**:
  - artists, albums, songs, playlists, genres
- **Providers (streaming integration)**:
  - sign-in and enablement inside the Music app for:
    - **Tidal**
    - **SoundCloud**
    - **Netease Cloud Music**
  - OAuth2 login via `svc.auth.oauth2.*`; streaming via typed stubs or bounded HTTP (`svc.net.http.request`)
  - capability-gated per provider (e.g. `cloud.music.stream.tidal`); audit logs (no secrets logged)
- **Playback**:
  - opens single track in Music Player (or inline playback)
  - queue management (play next, add to queue) + “Up Next” surface
  - shuffle/repeat modes
- **Casting** (optional):
  - AirPlay-style remote playback via `svc.bus.call(session, "media.play", ...)`
  - capability-gated: `dsoftbus.media.cast`
- **Cloud sync** (optional):
  - sync playlists/favorites via `svc.cloud.sync.*`
  - capability-gated: `cloud.sync.music`

**Status**: ❌ **Not yet planned**

**Action needed**: Create new task `TASK-XXXX-music-library-app-streaming-cloud.md`

---

### 5. Video Player (single-file player)

**Scope**: play a single video file (MP4/MKV/WEBM/GIF/APNG/MJPEG).

**Features**:
- open via picker or "Open With"
- play/pause/seek/volume
- fullscreen mode
- subtitle support (SRT/VTT)
- frame export as PNG (optional)

**Status**: ⚠️ **Partially planned** in `tasks/TASK-0102-ui-v16d-music-video-apps-os-proofs.md` (but only GIF/APNG/MJPEG, no real video codecs)

**Action needed**: Extend TASK-0102 or create new task for full video player (MP4/H264/etc.).

---

### 6. TV (hub app, like Apple TV)

**Scope**: Apple TV-style hub with **Watch Now**, **Library (curated)**, **Providers**, and **Search**.

**Features**:
- **Tabs / IA**:
  - **Watch Now**: up next / continue watching / recommendations (provider-sourced initially; hub-composed later)
  - **Library**: curated local videos (user-added), plus watch history/favorites
  - **Providers**: channel store + provider pages (Rakuten, ZDF, Mubi, Red Bull TV, …)
  - **Search**: across enabled providers + curated library
- **Curated local Library (no auto-index)**:
  - user adds videos explicitly (“Add to TV Library”) from Files/Photos/share sheet
  - keep camera/screen recordings in Photos by default; optionally add them to TV
  - generate thumbnails on-demand with strict budgets
- **Provider integration**:
  - provider enablement/install inside TV (“Channels” store)
  - OAuth2 login via `svc.auth.oauth2.*`; streaming via typed stubs or bounded HTTP (`svc.net.http.request`)
  - capability-gated per provider (e.g. `cloud.video.stream.youtube`); audit logs (no secrets logged)
- **Playback**:
  - opens single video in Video Player (or inline playback)
  - resume from last position
  - subtitle selection
- **Casting** (optional):
  - Chromecast-style remote playback via `svc.bus.call(session, "media.play", ...)`
  - capability-gated: `dsoftbus.media.cast`
- **Cloud sync** (optional):
  - sync watch history/favorites via `svc.cloud.sync.*`
  - capability-gated: `cloud.sync.videos`

**Status**: ❌ **Not yet planned**

**Action needed**: Create new task `TASK-XXXX-tv-app-hub-providers-library.md`

---

## Media Indexer Service (`mediad`)

**Why a service?**

Instead of each app scanning storage independently:
- **Single authority**: `mediad` owns indexing and metadata extraction.
- **Shared cache**: all apps query the same index (no duplicate work).
- **Bounded resources**: scanning is rate-limited and budgeted (no "scan storm" on boot).
- **Deterministic proofs**: host-first tests for indexing logic; QEMU markers for OS integration.

**API surface** (IDL-based):

```capnp
interface MediaIndexer {
  scanPath @0 (path :Text, kind :MediaKind) -> (jobId :UInt64);
  queryPhotos @1 (filter :PhotoFilter) -> (results :List(PhotoMetadata));
  queryMusic @2 (filter :MusicFilter) -> (results :List(MusicMetadata));
  getThumbnail @3 (uri :Text) -> (thumbnail :Data);
  // Note: TV does not require Kodi-style auto-index. Curated TV Library can store metadata per entry,
  // and only use `getThumbnail` / lightweight metadata extraction on-demand.
}

enum MediaKind {
  photos @0;
  music @1;
}

struct PhotoMetadata {
  uri @0 :Text;
  dateTime @1 :UInt64;  # Unix timestamp
  location @2 :Location;  # optional GPS
  cameraModel @3 :Text;  # optional EXIF
  width @4 :UInt32;
  height @5 :UInt32;
}

struct MusicMetadata {
  uri @0 :Text;
  title @1 :Text;
  artist @2 :Text;
  album @3 :Text;
  genre @4 :Text;
  year @5 :UInt16;
  duration @6 :UInt32;  # seconds
  coverArt @7 :Data;  # optional embedded cover
}

// Video metadata indexing is intentionally not part of v0 `mediad` in this track.
// If we add it later, it must remain optional and explicitly enabled, to preserve the curated TV Library stance.
```

**Capability gates**:
- `media.index.scan` (trigger indexing; system/apps only)
- `media.index.query` (query index; apps only)
- `media.index.thumbnail` (generate thumbnails; apps only)

**Constraints**:
- Bounded scan rate: max N files/sec, max M MB/sec.
- Bounded metadata size: cap EXIF/ID3 tag sizes.
- Bounded thumbnail cache: LRU eviction, max total size.
- Deterministic ordering: stable sort by date/name.

---

## Streaming Connectors (pluggable providers)

**Why pluggable?**

Instead of hardcoding Tidal/SoundCloud/Netease into the Music app:
- **Ecosystem-friendly**: third parties can ship providers for new services.
- **Security**: providers are signed bundles; tokens are held by `authd` (not apps).
- **Consistent UX**: all providers use the same OAuth2/account flow (via NexusNet SDK).

**Provider model** (recommended):

- **Provider bundle** (signed, installable) registers a `streaming_provider_id` and declares:
  - supported media kind(s): music / video
  - supported auth protocol: OAuth2 / OIDC / API key
  - supported scope families and UX strings
  - API endpoints (REST/GraphQL/typed stubs)
  - minimum policy requirements
- The **Accounts UI** (Settings) enumerates installed providers and runs the provider-owned auth flow.
- The **auth authority** (`authd`) holds refresh tokens for `(user, provider, account)` and issues **short-lived access tokens** on demand.
- Apps (Music/Videos) request grants against `(provider, account, scopes)`; grants remain **per-app** and revocable.

**Capability gates** (per provider):
- `cloud.music.stream.tidal`
- `cloud.music.stream.soundcloud`
- `cloud.music.stream.netease`
- `cloud.video.stream.youtube`
- `cloud.video.stream.netflix` (example)

**Security guardrails** (non-negotiable):
- Providers must be **signed** and policy-approved to register globally.
- Tokens are secrets: never logged; stored only in keystore/keychain namespaces.
- No "global token visibility": apps never enumerate or read tokens; they only get scoped access.
- Provider UX must prevent phishing (strict redirect allowlists, bounded timeouts, clear UI indicators).

---

## Casting / Device streaming (DSoftBus-based remote playback)

**Why DSoftBus?**

Instead of AirPlay/Chromecast vendor lock-in:
- **Open protocol**: any device can implement the casting receiver contract.
- **Capability-gated**: `dsoftbus.media.cast` (sender) and `dsoftbus.media.receive` (receiver).
- **Media session sync**: play/pause/seek state is synchronized via DSoftBus streams.

**API surface** (IDL-based; v0 favors Cast; file streaming is optional later):

```capnp
interface MediaCast {
  discover @0 () -> (receivers :List(Receiver));
  connect @1 (receiverId :Text) -> (session :MediaSession);
  play @2 (session :MediaSession, uri :Text, metadata :MediaMetadata) -> ();
  pause @3 (session :MediaSession) -> ();
  seek @4 (session :MediaSession, position :UInt32) -> ();
  stop @5 (session :MediaSession) -> ();
}

struct Receiver {
  id @0 :Text;
  name @1 :Text;
  kind @2 :ReceiverKind;
}

enum ReceiverKind {
  audio @0;  # speakers/headphones
  video @1;  # TV/monitor
  both @2;   # full media receiver
}
```

**Capability gates**:
- `dsoftbus.media.cast` (send media to remote receiver)
- `dsoftbus.media.receive` (act as a receiver)

**Constraints**:
- Bounded discovery: max N receivers, max M discovery time.
- Bounded session count: max K active cast sessions per device.
- Deterministic state sync: media session state is bounded and versioned.

---

## Cloud Sync (optional iCloud-style sync)

**Why optional?**

- **Local-first by default**: apps work fully offline.
- **Cloud is additive**: users opt-in to sync (capability-gated).
- **No vendor lock-in**: sync protocol is open (can self-host or use third-party providers).

**Sync model** (recommended):

- **Sync engine** (`syncd` or part of `mediad`) handles:
  - op-log sync for playlists/favorites/watch history (not raw files)
  - conflict resolution (explicit; no silent merges)
  - bounded retry budget (no unbounded exponential backoff)
- **Sync providers** (pluggable):
  - iCloud-style (Apple)
  - Google Drive-style (Google)
  - self-hosted (WebDAV/S3-compatible)
- **Capability gates**:
  - `cloud.sync.photos`
  - `cloud.sync.music`
  - `cloud.sync.videos`

**Security guardrails**:
- Sync tokens are secrets: never logged; stored only in keystore.
- Sync is auditable: all sync events are logged (without secrets).
- Sync is bounded: max N MB/day, max M sync operations/hour.

---

## Phase map (what "done" means by phase)

### Phase 0 (single-file viewers/players + host proofs)

- ✅ Image Viewer (TASK-0090)
- ✅ Music Player (TASK-0102)
- ⚠️ Video Player (TASK-0102, but only GIF/APNG/MJPEG)

**Action needed**: Extend Video Player to support real video codecs (MP4/H264/etc.).

---

### Phase 1 (library apps + local indexing)

- **Photos library**:
  - scan `state:/pictures/` recursively
  - extract EXIF metadata
  - timeline/albums views
  - search by date/location/camera
- **Music library**:
  - scan `state:/music/` recursively
  - extract ID3/Vorbis tags
  - artists/albums/songs/playlists views
  - search by artist/album/genre
- **TV app (hub + curated library)**:
  - Watch Now + Providers + Search + curated Library
  - “Add to TV Library” flow from Files/Photos
  - optional DSoftBus casting integration (device picker)

**Deliverables**:
- `mediad` service (indexer + query API)
- Photos app (library mode)
- Music app (library mode)
- TV app (hub mode)
- Host tests for indexing/metadata extraction
- QEMU markers for OS integration

---

### Phase 2 (streaming + cloud sync)

- **Streaming connectors**:
  - Tidal provider (music)
  - SoundCloud provider (music)
  - Netease provider (music)
  - YouTube provider (video)
- **Cloud sync**:
  - iCloud-style sync for playlists/favorites/watch history
  - pluggable sync providers (Google Drive/self-hosted)
- **Casting**:
  - DSoftBus-based remote playback (AirPlay/Chromecast-style)
  - media session sync (play/pause/seek state)

**Deliverables**:
- Streaming provider bundles (signed, installable)
- OAuth2/account integration (via NexusNet SDK)
- Sync engine (`syncd` or part of `mediad`)
- Casting protocol (IDL + DSoftBus streams)
- Host tests for streaming/sync/casting (deterministic)
- QEMU markers for OS integration

---

### Phase 3 (pro features)

- **Smart search**:
  - ML-based face grouping (Photos)
  - smart playlists (Music)
  - recommendations (Music/Videos)
- **Editing**:
  - photo editing (crop/rotate/filters)
  - video editing (trim/merge/effects)
  - audio editing (trim/fade/normalize)
- **Advanced sync**:
  - full-resolution photo sync (not just metadata)
  - offline downloads for streaming content (policy-gated)

---

## Candidate subtasks (to be extracted into real tasks)

### Photos

- **CAND-MEDIA-APP-010: Photos library v1 (indexing + timeline + albums + search)**
  - scan `state:/pictures/` recursively
  - extract EXIF metadata (date/location/camera)
  - timeline view (by date)
  - albums view (user-created + smart albums)
  - search by date/location/camera/tags
  - proof: deterministic host tests for indexing/metadata extraction; QEMU markers for OS integration

- **CAND-MEDIA-APP-011: Photos cloud sync v1 (iCloud-style sync for metadata + thumbnails)**
  - sync metadata/thumbnails via `svc.cloud.sync.*`
  - capability-gated: `cloud.sync.photos`
  - audit logs for sync events
  - proof: deterministic host tests for sync logic; QEMU markers for OS integration

- **CAND-MEDIA-APP-012: Photos smart search v1 (ML-based face grouping + smart albums)**
  - face detection/grouping (optional ML)
  - smart albums (e.g., "photos from last summer")
  - proof: deterministic host tests for face grouping; QEMU markers for OS integration

### Music

- **CAND-MEDIA-APP-020: Music library v1 (indexing + artists/albums/songs + playlists)**
  - scan `state:/music/` recursively
  - extract ID3/Vorbis tags (artist/album/genre/year/cover art)
  - artists/albums/songs/playlists views
  - search by artist/album/genre
  - proof: deterministic host tests for indexing/metadata extraction; QEMU markers for OS integration

- **CAND-MEDIA-APP-021: Music streaming v1 (Tidal + SoundCloud + Netease providers)**
  - OAuth2 login via `svc.auth.oauth2.*`
  - stream via `svc.net.http.request` or typed stubs
  - capability-gated: `cloud.music.stream.tidal`, `cloud.music.stream.soundcloud`, `cloud.music.stream.netease`
  - audit logs for streaming events (no secrets logged)
  - proof: deterministic host tests for streaming logic (mocked backends); QEMU markers for OS integration

- **CAND-MEDIA-APP-022: Music cloud sync v1 (playlists + favorites sync)**
  - sync playlists/favorites via `svc.cloud.sync.*`
  - capability-gated: `cloud.sync.music`
  - audit logs for sync events
  - proof: deterministic host tests for sync logic; QEMU markers for OS integration

- **CAND-MEDIA-APP-023: Music casting v1 (AirPlay-style remote playback via DSoftBus)**
  - discover receivers via `svc.bus.discover`
  - connect and play via `svc.bus.call(session, "media.play", ...)`
  - capability-gated: `dsoftbus.media.cast`
  - proof: deterministic host tests for casting logic (localSim); QEMU markers for OS integration

### TV

- **CAND-MEDIA-APP-030: TV app v1 (Watch Now + curated Library + Providers + Search)**
  - Watch Now feed + Continue Watching
  - curated Library (user-added items only; no auto-index of captures)
  - Providers store + provider pages + login
  - Search across enabled providers + curated Library
  - proof: deterministic host tests for library + provider stubs; QEMU markers for OS integration

- **CAND-MEDIA-APP-031: TV providers v1 (YouTube + “channels” like Rakuten/ZDF/Mubi/Red Bull TV)**
  - OAuth2 login via `svc.auth.oauth2.*`
  - stream via typed stubs or REST API
  - capability-gated: `cloud.video.stream.youtube`, `cloud.video.stream.netflix`
  - audit logs for streaming events (no secrets logged)
  - proof: deterministic host tests for streaming logic (mocked backends); QEMU markers for OS integration

- **CAND-MEDIA-APP-032: TV cloud sync v1 (watch history + favorites + library metadata sync)**
  - sync watch history/favorites via `svc.cloud.sync.*`
  - capability-gated: `cloud.sync.videos`
  - audit logs for sync events
  - proof: deterministic host tests for sync logic; QEMU markers for OS integration

- **CAND-MEDIA-APP-033: TV casting v1 (Chromecast-style remote playback via DSoftBus)**
  - discover receivers via `svc.bus.discover`
  - connect and play via `svc.bus.call(session, "media.play", ...)`
  - capability-gated: `dsoftbus.media.cast`
  - proof: deterministic host tests for casting logic (localSim); QEMU markers for OS integration

### Shared infrastructure

- **CAND-MEDIA-APP-040: Media indexer service (`mediad`) v1 (scan + metadata + query API)**
  - scan `state:/pictures/` and `state:/music/` recursively
  - extract metadata (EXIF/ID3 tags)
  - provide query API (by artist/album/date/etc.)
  - generate thumbnails via `thumbd`
  - capability-gated: `media.index.scan`, `media.index.query`, `media.index.thumbnail`
  - proof: deterministic host tests for indexing/metadata extraction; QEMU markers for OS integration

- **CAND-MEDIA-APP-041: Streaming provider framework v1 (pluggable OAuth2 + API stubs)**
  - provider bundle registration (signed, installable)
  - OAuth2/OIDC flow via `svc.auth.oauth2.*`
  - token management via `authd` (no raw tokens in apps)
  - capability-gated: `account.provider.register`, `account.use`
  - proof: deterministic host tests for provider registration/auth; QEMU markers for OS integration

- **CAND-MEDIA-APP-042: Media casting protocol v1 (DSoftBus-based remote playback)**
  - IDL-based casting API (discover/connect/play/pause/seek/stop)
  - media session sync via DSoftBus streams
  - capability-gated: `dsoftbus.media.cast`, `dsoftbus.media.receive`
  - proof: deterministic host tests for casting logic (localSim); QEMU markers for OS integration

- **CAND-MEDIA-APP-043: Media sync engine v1 (op-log sync for playlists/favorites/watch history)**
  - sync engine (`syncd` or part of `mediad`)
  - op-log sync (not raw files)
  - conflict resolution (explicit; no silent merges)
  - bounded retry budget
  - capability-gated: `cloud.sync.photos`, `cloud.sync.music`, `cloud.sync.videos`
  - proof: deterministic host tests for sync logic; QEMU markers for OS integration

---

## Extraction rules (how candidates become real tasks)

A candidate becomes a real `TASK-XXXX` only when:

- it is implementable under current gates (or explicitly creates prerequisites),
- it has **proof** (deterministic host tests and/or QEMU markers where valid),
- it declares what is *stubbed* (explicitly) vs. what is real,
- it names the authority boundary (service vs library vs SDK) and does not create a competing authority,
- it documents security invariants (no secrets in logs, capability-gated operations, bounded resources).

---

## Security checklist (for security-relevant code)

When touching streaming/cloud/casting/sync:

- [ ] Fill security section in task (threat model, invariants, DON'T DO)
- [ ] Write `test_reject_*` tests for negative cases (invalid tokens, oversized payloads, etc.)
- [ ] Add QEMU hardening markers proving enforcement
- [ ] No `unwrap`/`expect` on untrusted input (network responses, user-provided URIs, etc.)
- [ ] No secrets in logs or error messages (tokens, API keys, passwords)
- [ ] Audit records for security decisions (grant/revoke, sync events, streaming events)

---

## DON'T DO (Hard Failures)

### Security

- DON'T skip identity verification even for localhost (use kernel/transport identities, not payload strings)
- DON'T use "warn and continue" on auth failures (fail closed)
- DON'T duplicate policy logic outside `policyd` (single authority)
- DON'T expose tokens/secrets via any API (use handles/session-bound credentials)
- DON'T accept unbounded input sizes (cap metadata/thumbnails/streaming payloads)
- DON'T log secrets (tokens, API keys, passwords)

### Architecture

- DON'T hardcode streaming providers into apps (use pluggable provider framework)
- DON'T create parallel indexing authorities (single `mediad` service)
- DON'T bypass capability gates for "convenience" (all streaming/cloud/casting operations are gated)
- DON'T add dependencies outside license allowlist (Apache-2.0, MIT, BSD-2/3)
- DON'T rename packages or folders without approval

---

## Summary: App matrix

| Media Type | Single-file viewer/player | Library / hub app | Status |
| ---------- | ------------------------- | ----------------- | ------ |
| **Images** | Image Viewer (TASK-0090) ✅ | Photos (extend TASK-0106) ⚠️ | **Extend TASK-0106** |
| **Music** | Music Player (TASK-0102) ✅ | Music (new task) ❌ | **Create CAND-MEDIA-APP-020** |
| **Video** | Video Player (TASK-0102) ⚠️ | TV (new task) ❌ | **Create CAND-MEDIA-APP-030** |

**Next steps**:

1. Extend TASK-0106 (Gallery → Photos library)
2. Create CAND-MEDIA-APP-020 (Music library + streaming)
3. Create CAND-MEDIA-APP-030 (TV hub + curated library + providers)
4. Create CAND-MEDIA-APP-040 (Media indexer service `mediad`)
5. Create CAND-MEDIA-APP-041 (Streaming provider framework)
