<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Screenshot / Screencap / Share Sheet

This doc ties together:

- screenshot capture (built — RFC-0095, ADR-0071, TASK-0068),
- screen recording (TASK-0105 — the film switch is drawn greyed out and takes no input until then),
- share sheet conventions (targets, grants, deterministic UX — TASK-0126..0128).

## The screenshot tool (TASK-0068)

The reference desktop's interactive screenshot tool, in the shell's glass.

| Key | What happens |
|---|---|
| Print | the screen FREEZES and the tool opens over the frozen frame |
| Shift+Print | the whole screen is saved at once, no tool |
| Alt+Print | the focused (front-most) window is saved at once, no tool |

Inside the tool:

- **Selection** (the default): drag anywhere to draw a rectangle; drag inside it to move it;
  drag a corner handle to resize it from the opposite corner. Outside the selection the frozen
  frame is dimmed by a translucent scrim (no blur).
- **Screen**: the whole frame.
- **Window**: every window is outlined where it was; a click picks one (the front-most is
  picked to start with). A window is captured as it showed — its visible rectangle, whatever
  overlapped it included.
- The panel at the bottom: the round X (also ESC) closes the tool and unfreezes the screen;
  photo/film (film is greyed out and disabled until recording exists); the shutter; the
  pointer toggle (draw the pointer
  into the image — off by default).

The image is a PNG in **Pictures → Screenshots**, named `<Screenshot from> <date> <time>.png`
(localized words, the local time; ` (2)` … when the name is taken). The system toast names the
file; a failure says so instead.

What you see is what is saved: the frame is read back BEFORE the tool appears, the tool is drawn
over it and is never in the image, and while the tool is up nothing on the screen moves (app
windows, overlays and the live shell content are taken out — the frozen frame shows them).

## Who may capture

- The `svc.screencap` route — the capability — is granted only to the `shell` and `settings`
  bundle types (`nexus.permission.SCREENCAP`, the packer's ceiling); no app can read the screen.
- windowd accepts the capture verb from screencapd's kernel identity alone, and refuses a
  freeze while the login screen owns the display (password fields are masked on screen; a
  per-surface secure flag for apps that show secrets unmasked is a follow-up).
- A freeze nobody ends unfreezes by itself after 60 seconds (at windowd's next wake).
- Nothing about a capture is logged except its kind and size — never a file name or pixels.

## Where the pieces live

| Piece | Owner |
|---|---|
| reading the display (GL front target, 2D/controller display plane) | gpud `OP_READBACK` |
| the freeze, the pointer out of the frame, the window rectangles | windowd `capture_gate` + `runtime/capture.rs` |
| crop, pointer, PNG, the file | screencapd (`png-encode`, vfsd `OP_ARM_VMO` + `OP_WRITE_VMO`) |
| the tool | the desktop shell (`ui/components/capture/`, `composables/capture.store.nx`) |

Copying a capture to the clipboard is TASK-0087's (clipboard flavors); an editor, a gallery and
the share sheet are out of scope here.
