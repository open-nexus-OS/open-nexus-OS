# 2026-11-03 — where a Super+Ctrl chord stops on the board (TASK-0348 P7)

The desk's keyboard `3434:0123` and the receiver `046d:c53f` (keyboard + mouse interfaces) on the
board's hub, our stack in the boot protocol (`../2026-10-04-usb-boot-protocol/`).

## Question

On board cycle 80 (2026-11-02) the operator pressed Super+Ctrl+W and Super+Ctrl+M and nothing
happened: no `windowd: wm chord (zone=` line, no `wm tile deny`. Letters typed into the IME probes
arrived, the same chords over QEMU's USB keyboard (`usb-visible`) tile every run. Which hop of
hidrawd → inputd → windowd loses the chord on the board, and does the keyboard send the modifier
at all?

## Instrument

The chord chain, string-identical in the services and the chain simulation
(`tools/nx/chains/markers.txt`, group `input-chord`), plus two per-second witness lines that carry
a modifier mask and counts — never a key (RFC-0113 §6):

| hop | line | says |
|---|---|---|
| K1 | `hidrawd: chain K1 keyboard events normalized` | a keyboard's events left hidrawd's parser |
| — | `hidrawd: keyboard (reports=N refused=N mods=0xMM)` | per second: boot reports, refusals, the OR of their modifier bytes (bit 0 LCtrl, 3 LGUI, 4 RCtrl, 7 RGUI) |
| — | `inputd: keys (mods=0xMM chords=N)` | per second: the surface wire's modifier bits any key event saw (1 Shift, 2 Ctrl, 4 Alt, 8 AltGr, 16 Super), chords recognized |
| K2 | `inputd: chain K2 chord recognized` | the registry matched a chord with exactly its modifiers |
| K3 | `inputd: chain K3 chord pushed to windowd` + `inputd: hops chord (code= report>push=…us)` | the fact went out, timed from the device's report |
| K4 | `windowd: chain K4 chord recv` | windowd got the fact; `wm chord` / `wm tile deny` say what it did |

Procedure: boot cycle 81, open an app, press Super+Ctrl+W, then Super+Ctrl+M, then
Super+Ctrl+Shift+Backspace, each once, a second apart, with the mouse moving a little between
them (the per-second lines print on the next USB wake). Read `build/logs/board--<ts>/uart.log`.

## Hypotheses the log decides

| | hypothesis | decided by |
|---|---|---|
| H1 | the keyboard sends no GUI modifier for the key the operator holds (a Win lock, a Mac/Win mode switch that puts Alt where Super is) | `hidrawd: keyboard (… mods=` never has bit 3 or 7; bit 2 (LAlt) set while Super is held → the mode switch |
| H2 | hidrawd drops the modifier events on the wire | hidrawd's mask has GUI, `inputd: keys (mods=` never has 16 |
| H3 | inputd sees Super+Ctrl but matches no chord (the layout's letter for the key, a modifier bit beside the two) | inputd's mask has 2 and 16, `chords=0`, no K2; the mask's extra bits name the stray modifier |
| H4 | the fact is recognized but lost before or on the push (voided on a dead push, overwritten) | K2 present, K3 absent, or `hops chord` without K4 |
| H5 | windowd gets the fact and refuses it by name | K4 present, `windowd: wm tile deny (reason=` |

## Results

Board cycle 81 (`build/logs/board--2026-11-03T10-38-18/uart.log`), chords pressed on the keyboard
`3434:0123` before any letter was typed:

| hop | seen | detail |
|---|---|---|
| K1 | yes | `hidrawd: chain K1 keyboard events normalized` |
| witness | yes | `hidrawd: keyboard (… mods=0x09)` on every chord second: LCtrl + LGUI, no stray bit; `refused=0` |
| K2 | yes | `inputd: keys (mods=0x12 chords=N)` — Ctrl + Super as the wire's bits; 25 chords recognized over the cycle (codes 1, 2, 9, 10, 14, 15) |
| K3 | yes | `inputd: hops chord (code= report>push=` 210–670 µs per chord |
| K4 | **no** | no `windowd: chain K4 chord recv`, no `wm chord`, no `wm tile deny` |

The keyboard route went live (`inputd: live keyboard route on`) only at line 3440, after the last
chord (line 3392): nothing but chords had been typed.

## Verdict

H1–H3 and H5 are out; H4 in its second form: the fact reached windowd and was dropped before it
was applied. `apply_input_state` mirrored the sampled fields into `self.state`, ran the wheel and
the capture key, then returned on `self.state == old_state` — and applied the chord only after
that return. A chord pressed with the pointer still, before any key was dispatched (so
`keyboard_visible` never toggles), changes nothing in the mirrored state: the one-shot fact fell
with the short-circuit. QEMU's injector types a letter first, so there the state always differed.
Fix: the chord runs with the other one-shot facts, before the short-circuit (TASK-0348 P7); the
pattern itself — a discrete edge riding a sampled state — is P17 (the key channel carries it).
Cycle 82 (`build/logs/board--2026-11-03T10-48-35/uart.log`) judged the fix green: K1–K4, then
`windowd: wm chord (zone=` for left-half, right-half, fill, return, close and minimize — 13 chords,
report→push 207–791 µs — with `board-visual: tile` and `board-visual: restore` acked at the monitor
and no FAIL line that cycle 80 did not have. The restart chord was not pressed (the operator finds
Super+Ctrl+Shift+Backspace too long a shortcut; its code takes the same fact path).
