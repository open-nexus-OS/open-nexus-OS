# ADR-0071: Screen capture — the readback is gpud's, the freeze is windowd's, the files are screencapd's, the UI is the shell's

- Status: Accepted
- Date: 2026-10-08
- Links:
  - Tasks: `tasks/TASK-0068-ui-v7c-screenshot-screencap-share-sheet.md`
  - RFCs: `docs/rfcs/RFC-0095-screen-capture-v1-readback-freeze-shell-ui.md` (the contract),
    `docs/rfcs/RFC-0093` (the probe-RT readback), `docs/rfcs/RFC-0053` (the capture key fact),
    `docs/rfcs/RFC-0072` (the binary write op)
  - Related ADRs: `docs/adr/0068-*` (modal semantics), `docs/adr/0069-*` (key chords via inputd,
    windowd draws nothing), `docs/adr/0070-*` (one authority per user-visible store)

## Context

The operator chose the reference desktop's interactive screenshot tool for the OS (2026-10-08):
Print freezes the screen, an overlay picks a selection, the whole screen or a window, a glass
panel holds the shutter, the result lands as a PNG in the Pictures folder. Four questions had to
be decided once: who reads pixels, who freezes the screen, who writes the file, who draws the UI.

## Decision

1. **gpud is the ONE readback authority** (`OP_READBACK`). Only gpud can read the GL front render
   target, and only through a copy into a backed resource (never a transfer from the scanout);
   the CPU paths (virtio 2D, the board's display controller) copy the display plane. The
   one-shot display-truth sample (`SELFTEST: display nonblack ok`) goes through the same
   function. windowd is gpud's only client, so every capture request reaches gpud through it.
2. **windowd owns the freeze.** The grabbed frame becomes the compositor's base layer; app and
   overlay windows are hidden while the shell's overlay is up; the cursor stays live. A freeze is
   asked for only by screencapd's kernel sid, is refused while the greeter runs, and ends by
   itself after a bound. windowd draws no capture UI.
3. **screencapd owns the capture session and the file.** It lends windowd its frame VMO once
   (allocated at start), crops the frozen frame row by row, draws the pointer windowd wrote
   behind the frame when asked, encodes PNG with bounded state into a worst-case-sized VMO and
   has vfsd write it in ONE transaction under `/Bilder/Screenshots`. It is reached only through
   the SDK route `svc.screencap`, which only shell and settings bundles may hold.
4. **The UI is the shell's** (DSL): the scrim, the selection, the mode buttons, the panel, the
   toast. The selection needs a drag gesture, which becomes a general DSL capability (host
   triggers `DragStart` / `DragMove` / `DragEnd` + `device.drag*`), not a screenshot special case.
5. **Keys follow ADR-0069:** inputd recognizes Print, Shift+Print and Alt+Print, swallows them,
   and hands windowd a one-shot fact; windowd tells the shell.

## Consequences

- One pixel path to keep honest: a fix to the readback (a backend, a cache rule) fixes the
  screenshot, the display-truth proof and any later consumer (recording, TASK-0105) at once.
- The freeze adds a compositor mode; it is bounded and owned by one sender, so a crashed or
  confused client cannot leave the screen frozen.
- vfsd gains its first binary write op (`OP_ARM_VMO` + `OP_WRITE_VMO`, pulled into nxfs's one
  reused working buffer — the per-write 64 KiB leak went with it); the files app benefits
  later (copy, import).
- The DSL's size modifiers honour Int expressions (`.width($state.x)`): the checker always
  accepted them, the emitter dropped them silently.
- The DSL gains a drag gesture any page can use.
- Capturing an app's hidden parts stays out: a window capture is what was on screen.

## Alternatives considered

- **windowd reads pixels itself on the CPU paths** — rejected: two readback paths (CPU and GL)
  would drift; gpud already owns the GL one.
- **The shell holds the frozen image** — rejected: the DSL has no image-from-memory source, and
  the compositor already composites a base layer.
- **A dedicated capture app at overlay level** — rejected: overlay surfaces are bottom bands
  today, and the shell already owns modal input above windows.
