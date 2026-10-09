# RFC-0095: Screen capture v1 — one readback authority, a frozen frame, the shell's screenshot UI

- Status: Done (2026-10-09 — Phases 0–3, TASK-0068; QEMU: Print → drag → shutter on
  `usb-visible`, the readback probe on every visible lane; board `dev-f94c6744`: the tool, a
  moved and corner-resized selection, Window mode → `saved (kind=window …)`, the toast —
  `board-visual: screenshot`; Phase 4 belongs to TASK-0105)
- Owners: @ui @runtime
- Created: 2026-10-08
- Last Updated: 2026-10-09
- Links:
  - Tasks: `tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md` (execution + proof)
  - ADRs: `docs/adr/0071-screen-capture-readback-in-gpud-freeze-in-windowd-ui-in-the-shell.md`
  - Related RFCs: RFC-0093 (display handoff — the probe-RT readback this generalizes),
    RFC-0053 (the visible state — the capture key fact, amended), RFC-0072 (VFS v2 — the
    binary write op, amended), RFC-0086 (window feed), RFC-0094 (clipboardd — image items are
    TASK-0087's), ADR-0068 (modal semantics), ADR-0069 (key chords via inputd)

## Status at a Glance

- **Phase 0 (contract)**: ✅ this RFC, ADR-0071, the RFC-0053 / RFC-0072 amendments
- **Phase 1 (readback + freeze)**: ✅ gpud `OP_READBACK`, windowd freeze/thaw, the selftest probe
- **Phase 2 (the service)**: ✅ screencapd — crop, pointer, PNG, `/Bilder/Screenshots`
- **Phase 3 (keys + UI)**: ✅ Print / Shift+Print / Alt+Print, the drag gesture, the shell overlay
  (QEMU 2026-10-09; board 2026-10-09 — Shift+Print / Alt+Print are host-proven, not yet
  exercised end to end)
- **Phase 4 (recording)**: ⬜ TASK-0105 — the film switch is drawn greyed out and disabled until then

Definition: "Complete" means the contract is defined and the proof gates are green.

## Scope boundaries (anti-drift)

- **This RFC owns**: the capture keys and their path, the readback op, the freeze, the capture
  service's wire and gate, the PNG output and its place, the screenshot UI's contract with its
  services, the DSL drag gesture the UI needs.
- **This RFC does NOT own**: recording (TASK-0105), editing, a gallery, the share sheet
  (TASK-0126–0128), image items in the clipboard (TASK-0087), per-window re-composition.

### Relationship to tasks (single execution truth)

TASK-0068 implements and proves every phase except Phase 4.

## Context

Nothing in the system can produce a screenshot. gpud already reads display truth through a
probe render target (RFC-0093 §5, `SELFTEST: display nonblack ok`), but only a 256×64 strip,
inside gpud. The operator chose the reference desktop's interactive screenshot tool (2026-10-08):
Print freezes the screen, an overlay picks a selection, the whole screen or a window, a panel at
the bottom holds the shutter, and the result is a PNG in the Pictures folder.

## Goals

- Print opens the screenshot UI over a FROZEN frame — what you see is what is saved, and the
  overlay can never appear in the image. Shift+Print saves the screen, Alt+Print the focused
  window, without the UI.
- ONE pixel-readback authority (gpud) for every backend: GL (QEMU virgl), virtio 2D, and the
  board's display controller.
- ONE capture facade (screencapd) that holds the frozen frame, crops it, draws the pointer when
  asked, encodes PNG and writes it to the user's `/Bilder/Screenshots`.
- The UI is the shell's (DSL), never windowd's or screencapd's.

## Non-Goals

Recording; an editor; a gallery; sharing; image clipboard items; capturing a window's hidden
parts (a window capture is its on-screen rectangle at the frozen moment, occluders included);
formats other than PNG; multi-monitor.

## Constraints / invariants (hard requirements)

- **Nothing frame-sized touches a heap.** Service heaps do not return large blocks, so
  screencapd allocates its frame VMO (the layout maximum `LAYOUT_MAX` = 1920 × 1080, BGRA, plus
  the 64 × 64 pointer sprite behind it) and its encoder scratch once at start. The PNG of one
  save goes into a VMO sized for the encoder's worst case (`png_encode::max_encoded_len`) and
  destroyed after the write — VMOs are page-backed and return their frames on destroy since
  TASK-0286. gpud's capture backing is the caller's VMO, never gpud's memory; frames are read
  row by row (`vmo_read`), never mapped whole.
- **The overlay is never captured**: the frame is grabbed before the overlay shows, and every
  crop is of that frame.
- **No capture at the greeter**: windowd refuses a freeze while the login phase is active.
  Password fields are masked on screen, so a capture cannot reveal them. A per-surface secure
  flag (an intent byte) is a follow-up for apps that show secrets unmasked.
- **Bounded**: crops are clipped to the frame; a rectangle outside it or of zero size is refused;
  file names are bounded (stem ≤ 64 bytes, no `/` or `\`, no control characters, no leading
  dot, no surrounding space); a freeze nobody thaws ends at windowd's first wake after
  `FREEZE_MAX_NS` (60 s); a readback gpud has not answered in `READ_MAX_NS` (2 s) fails its
  request and undoes the freeze it began.
- **Markers carry kinds and sizes only** — never pixels, file contents or names.

## Proposed design

### Contract / interface (normative)

**Keys.** HID usage `0x46` (Print) joins the keymap set; hidrawd maps evdev 99 to it on the
virtio lanes. inputd swallows Print, Shift+Print and Alt+Print — they never reach imed or an
app — and sets a one-shot `capture` fact in the visible state (RFC-0053 amendment: one appended
byte, `0` none, `1` the UI, `2` the screen, `3` the window). windowd applies the fact BEFORE its
unchanged-state early return and pushes `OP_SURFACE_CAPTURE_KEY = 30 {kind: u8, seq: u32}` to the
desktop surface, retained and re-sent until delivered (the window-feed pattern). app-host fires
the host trigger `CaptureOpen`, `CaptureScreen` or `CaptureWindow`. (Ops 30/31 were pencilled
for drag and drop; that work moved to TASK-0086 and takes the next free ops.)

**The shell's binding** (`svc.screencap`, permission `nexus.permission.SCREENCAP`, bundle types
`shell` and `settings` only):

| Method | Result | Effect |
|---|---|---|
| `begin()` | `CaptureFrame { w, h, front: CaptureWindow, windows: List<CaptureWindow { id, x, y, w, h }> }` | grab + freeze |
| `shoot(kind, x, y, w, h, pointer, stem)` | `Str` (the file name) | crop, save, thaw |
| `cancel()` | `Bool` | thaw |
| `shot(kind, pointer, stem)` | `Str` | grab, save, no UI (`screen` / `window` = the focused one) |

`kind` is `"area"`, `"screen"` or `"window"`; for `window`, `x` carries the window id. The
windows are their on-screen rectangles (clipped; an off-screen one is dropped), delivered BACK
TO FRONT — a page that draws them in order lets the front-most win a click where they overlap
— and `front` is the front-most one (id 0: none). The page passes only the localized stem
words; the binding appends the local time (` YYYY-MM-DD HH-MM-SS`, the zone of the region
push) when the clock and the zone are known, and screencapd appends `.png` (` (n)` when taken).

**screencapd's wire** (`nexus_wire::screencapd`, envelope `'S','C'` v1): `BEGIN` (reply: `w`,
`h`, the windows packed `{id:u32, x:i16, y:i16, w:u16, h:u16}` front to back), `SHOOT {mode, x:u32,
y, w, h, stem}` and `SHOT {mode, stem}` (reply: the saved file name), `CANCEL`, `PROBE {x, y, w,
h}` (reply: the brightest pixel as one `u32` — the selftest's proof, never pixels). `mode` is the
kind (`1` area, `2` screen, `3` window) with bit 7 set when the pointer is drawn. Statuses: OK,
MALFORMED, DENIED (the greeter), BUSY (a capture already running, none to shoot or cancel),
FAILED (no display, a readback failure), STORAGE (the file could not be written).

**screencapd → windowd**: `OP_SURFACE_CAPTURE = 32` on windowd's surface endpoint, accepted only
from screencapd's kernel sid. `cmd 4 = ATTACH` lends screencapd's frame VMO once at start (it
moves in, fire-and-forget: a message carries one capability, so a request with a reply cap has
no slot left for a VMO; a later attach replaces it, a restarted screencapd attaches again).
`cmd 1 = FREEZE`, `cmd 2 = THAW` and `cmd 3 = PROBE` carry screencapd's reply cap, which windowd
parks until gpud answers. A freeze first takes the pointer out of the frame on the paths that
draw it in — the GL build-up (an `OP_MOVE_CURSOR` to a negative position: "no pointer in this
frame") and the software blend (`BlendCursor` skipped); a hardware overlay is never in the frame
— and waits until a present composed without it was issued; then it asks gpud to read the whole
display into the VMO with the freeze flag. From the moment that readback is in gpud's queue the
screen is frozen: every present behind it lands over the frozen base, app windows and overlay
surfaces leave the composition, plane 1 is not re-rendered, and the pointer returns. The reply is
`{status, nonce, w, h, pointer x/y, hot x/y, sprite w/h, n, windows[n ≤ 8] {id, x, y, w, h}}`;
windowd writes the sprite it shows (premultiplied BGRA, ≤ 64 × 64) behind the frame in the VMO,
so screencapd keeps no second copy of the cursors. `THAW` re-uploads the wallpaper texture (GL,
`OP_WALLPAPER_DIRTY`, retried until gpud's queue takes it) and re-renders plane 1 (CPU paths).
`PROBE` reads a rectangle without freezing.

**windowd → gpud**: `OP_READBACK = 15 {x, y, w, h: u16, flags: u8}` with the destination VMO moved
in; rows land tight (`w × 4`). GL: the front render target is copied (`RESOURCE_COPY_REGION`) into
a capture resource backed by the moved VMO, then transferred back — the scanout is never a
transfer source (RFC-0093 §5). 2D and the display controller: a CPU copy of the display plane.
`flags & FREEZE`: the frame also becomes the base layer (GL: copied into the wallpaper texture
until the next wallpaper upload; CPU: copied into the retained plane). Reply `[status, 15]`
(two bytes — never the 1-byte or ≥5-byte shapes windowd reads as cursor or present replies).
`SELFTEST: display nonblack ok` is re-issued through this one readback function.

**screencapd → vfsd**: `OP_MKDIR` (an existing folder is fine), an exclusive `OP_CREATE` (a taken
name moves on to ` (2)`, ` (3)` … ` (99)`), then the two-message VMO write of the RFC-0072
amendment: `OP_ARM_VMO = 15` moves a plain clone of the PNG's VMO in (keyed by the kernel sender
id, no reply), `OP_WRITE_VMO = 14 {path, len}` follows on the reply inbox and writes the file in
ONE nxfs transaction, pulling the bytes with `vmo_read` into the store's reused working buffer;
vfsd closes the VMO before it answers. A failure after the create removes the claimed name. The
PNG is encoded row by row (each row read from the frame VMO, the pointer blended in) into the
VMO; nothing file-sized is ever on a heap. (A binary write in 8 KiB IPC frames was rejected:
every nxfs write was a transaction, and vfsd's heap paid 64 KiB per write until this task gave
nxfs one reused working buffer.)

**The DSL drag gesture** (the selection needs it): windowd sends `INPUT_KIND_DRAG = 5` instead of
`MOVE` while the primary button is held and `INPUT_KIND_RELEASE = 6` on its release, to the
surface that took the press (wherever the pointer is). The press itself is the ordinary `TAP`.
Every applied input sample (one per windowd loop pass) goes out as a DRAG frame — one attempt;
a frame the surface's full queue refused is owed and sent again on windowd's next pass, so the
last position always arrives without a clock. The RELEASE is discrete like a tap — windowd parks
until the surface takes it — and carries the gesture's last position. app-host only NOTES a DRAG
and computes the newest position once, right before the frame that shows it (positions that
arrive while a present is in flight are superseded, never dispatched); the repaint is the layout
diff's rows, tight for geometry-only changes. app-host arms a drag on the press; once the
pointer moved 3 px it fires
`DragStart` hit-tested at the PRESS point (modal-confined; a panel absorbs with its own
`DragStart -> Noop`) and a first `DragMove`, and `DragMove` / `DragEnd` then reach THAT box
wherever the pointer goes (`View::fire_on_box`); the release first moves the drag to where the
pointer let go, so a drag whose motion frames a busy surface lost still starts, moves and ends
there. `device.dragX`, `device.dragY`, `device.dragStartX`, `device.dragStartY`
(surface pixels, Int) are read by the reducer at dispatch time. The selection's geometry follows
the store through `.width/.height($state.…)` — a LAYOUT dependency (before this RFC the checker
accepted that form and the emitter dropped it silently).

**The UI** (desktop shell): Print opens a modal over the frozen frame — a translucent dark scrim
outside the selection (no blur), the selection with corner handles (drag to draw, drag inside to
move, drag a handle to resize), and a glass panel at the bottom centre styled like the shell's
notifications with the round X at its top right: three round mode buttons (Selection, Screen,
Window) above; the photo/film switch (film greyed out and disabled — `.disabled(true)`, no press
or hover — until Phase 4), the shutter and the pointer toggle below.
Window mode outlines the windows `begin()` returned, in place; a click picks one (the front-most
is picked when the tool opens). ESC (the modal's `Dismiss`) and the X cancel. After a save the
shell shows its system toast with the file name. While the tool is up the shell draws nothing
of its live content: the frozen frame already shows it, and a live top bar would sit over its
own frozen copy. (Keys inside the tool beyond ESC — Enter/Space to shoot, S/C/W for the modes,
P for the pointer — are a follow-up: the DSL has no key trigger for a surface without a text
field yet; ESC reaches it through the modal's dismiss path.)

### Phases / milestones (contract-level)

See "Status at a Glance". Additive evolution only: new kinds, flags and fields append.

## Security considerations

- **Threat**: an app captures the screen. **Mitigation**: the binding is shell/settings-only by
  bundle type; screencapd's routes come only from the SDK table; windowd accepts capture
  commands from screencapd's kernel sid alone.
- **Threat**: a login secret is captured. **Mitigation**: no freeze while the greeter runs;
  password fields are masked on screen.
- **Threat**: a frozen screen traps the user. **Mitigation**: ESC and the X cancel; windowd thaws
  by itself at its first wake after `FREEZE_MAX_NS` (a dead screencapd included); a readback
  gpud never answers fails after `READ_MAX_NS` and undoes the freeze.
- **Threat**: a crafted file name escapes the folder. **Mitigation**: the stem is validated and
  the folder is fixed.
- **Proof**: `test_reject_*` for every bound — the rectangle, the stem, a foreign sender, a
  freeze at the greeter, a double freeze, a thaw without a freeze.

## Failure model (normative)

- screencapd down: `svc.screencap.*` answers unavailable; the shell shows no overlay.
- A readback that fails (GL ring timeout, no device): `begin()` fails, nothing freezes.
- vfsd refuses or fills up: `shoot()` fails, the freeze still ends; nothing half-written is left
  under the final name (the file is written under its final name only after `OP_CREATE`
  succeeded; a failed write removes it).

## Proof / validation strategy (required)

### Proof (Host)

The PNG encoder (round trips, bounds), screencapd's crop / pointer / naming core, the wire
codecs with reject matrices, windowd's freeze state machine and capture gate, inputd's capture
key, the keymap's Print, the DSL drag triggers, the shell's conformance and goldens.

### Proof (OS/QEMU)

Every visible lane (GL and 2D): `screencapd: ready`, `SELFTEST: ui v7 screencap ok` (once the
desktop is up the selftest reads a strip at the centre through screencapd's PROBE and finds it
non-black); gpud's `SELFTEST: display nonblack ok` reads through the same copy function
(`copy_front_to_guest`).
`usb-visible`: the injector presses Print, drags a selection, presses the shutter —
`screencapd: freeze ok`, `screencapd: saved (kind=area …)`, `SELFTEST: ui v7 screenshot ok`.
Board: the operator rung `board-visual: screenshot`.

### Deterministic markers (if applicable)

`screencapd: ready` · `screencapd: freeze ok (w=… h=… windows=…)` · `screencapd: saved
(kind=… w=… h=… bytes=…)` · `screencapd: deny (reason=…)` · `SELFTEST: ui v7 screencap ok` ·
`SELFTEST: ui v7 screenshot ok` · `windowd: capture freeze on` / `off` / `expired` ·
`windowd: capture deny (reason=…)`. The capture lines are bounded per boot (screencapd: the
first eight of each kind; windowd: one per refusal reason), so neither a user taking many
screenshots nor a foreign sender floods the log.

## Alternatives considered

- **Capture live, hide the overlay for one frame** — rejected: the image is not what the user
  framed, and hiding then re-showing the overlay races the present.
- **The shell shows the frozen image as a picture** — rejected: the DSL has no image-from-memory
  source, and the compositor already has a base layer to put it in.
- **Read the scanout render target directly on GL** — rejected: a transfer from the scanout
  desynchronizes the host's GL present (RFC-0093 §5).
- **miniz_oxide for PNG** — rejected for v1: its compressor state is large for a heap that never
  frees; a fixed-Huffman encoder with bounded state is enough for screen content.

## Open questions

- A per-surface secure flag for apps that show secrets unmasked (an intent byte).
- Image items in the clipboard (TASK-0087) so a capture can be pasted at once.

## Implementation Checklist

- [x] RFC-0053 / RFC-0072 amendments · [x] png-encode (`encode_rows`, `max_encoded_len`)
- [x] gpud `OP_READBACK` · [x] windowd freeze/thaw/probe (`capture_gate`, `runtime/capture.rs`)
- [x] screencapd + topology/policy/volume · [x] vfsd `OP_ARM_VMO` + `OP_WRITE_VMO`, nxfs
  `write_from` (one transaction, one reused working buffer)
- [x] Print keys · [x] drag gesture (the release discrete, carrying the last position) · [x] the
  shell's overlay + toast · [x] lanes (`SELFTEST: ui v7 screencap ok` on every visible lane;
  Print → drag → shutter on `usb-visible`, 2026-10-09) · [x] board rung `board-visual: screenshot`
  (`dev-f94c6744`, 2026-10-09)
