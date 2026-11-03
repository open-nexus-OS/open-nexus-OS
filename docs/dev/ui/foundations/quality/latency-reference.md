<!-- Copyright 2026 Open Nexus OS Contributors -->
<!-- SPDX-License-Identifier: Apache-2.0 -->

# Latency and motion reference numbers

The numbers the UX gates measure against (RFC-0108, TASK-0351, TASK-0354, TASK-0355), researched
on the web on 2026-11-03 for TASK-0353 P1. Each number names at least two sources. Vendor
documentation is described, not linked, by project rule (no company or product names in the repo);
the operator holds the links in the planning conversation of that day.

| Quantity | Reference value | Our gate | Sources |
|---|---|---|---|
| Display refresh of the reference laptop (2026, entry class) | 60 Hz fixed, no adaptive refresh; one external display at 4K 60 Hz | frame budget 16.7 ms, p99; 0 of 32 frames dropped per transition | the vendor's spec page and a community spec wiki (both: 60 Hz, no adaptive refresh); a laptop-spec aggregator (60 Hz, adaptive refresh: no) |
| Touch to first changed frame on the reference phone class | 55 ms (2013 high-speed-camera study, 240 fps; the best other phone 114 ms); two frames at 60 Hz, about 32 ms, as the vendor's stated touch latency (2015 developer talk) | tap to first changed frame <= 50 ms (red above 100 ms) | the camera study as reported by a technology news site; the developer-forum thread quoting the two-frame figure |
| Perception threshold of pointer latency | about 60 ms mean (range 34–137 ms) for a mouse task; users notice slowness between 20 and 80 ms; inking needs about 50 ms, 20 ms to feel responsive | the 50 ms tap gate sits under the mean threshold | [Forch et al., "Are 100 ms fast enough?"](https://research.uni-luebeck.de/en/publications/are-100-ms-fast-enough-characterizing-latency-perception-threshol/); [Ink & Switch, "Slow software"](https://www.inkandswitch.com/slow-software); [Annett et al. 2014 (inking latency)](https://webdocs.cs.ualberta.ca/~wfb/publications/C-2014-GI-Latency.pdf) |
| Keypress to glyph on the screen, whole system | 30–50 ms on 1980s machines; 50–150 ms on current desktops with a 144 Hz display; the display alone adds half a frame plus its own delay | typing: input to present <= 50 ms (RFC-0108) | [Luu, "Computer latency 1977–2017"](https://danluu.com/input-lag/); [Luu, "Keyboard latency"](https://danluu.com/keyboard-latency/) |
| Implicit transition duration of the reference desktop's animation framework | 0.25 s default per transaction; window frame changes animate with that default | open/close of panels and windows 200–300 ms | the vendor's animation-framework guide ("default duration 0.25 s or the current transaction's"); a university lecture deck quoting the same default and a 0.5 s override example |
| Default spring of the reference UI toolkit (2023 and later) | response 0.55 s, damping fraction 1.0 (no bounce), blend 0; presets "smooth" (no bounce), "snappy" (slight bounce), "bouncy" | motion tokens are springs: the default critically damped, a snappy variant with slight bounce; every token settles within 550 ms | the vendor's toolkit reference for the default animation and the spring type; two third-party guides describing the duration/bounce form and the presets |
| Motion duration scale of the open mobile design system (Android) | short 50–200 ms, medium 250–400 ms, long 450–600 ms, extra long 700–1000 ms; "emphasized" is an easing curve, not a duration | micro-motion (press, wash, fade) 50–200 ms; panel and launcher 250–400 ms; nothing above 600 ms | two token ports on a Rust package registry mirroring the design system's motion tokens; a Flutter port listing the 16 duration tokens |

## What the numbers decide

- **The frame budget stays 16.7 ms.** The reference laptop's panel is 60 Hz, so a frame dropped at
  60 Hz is the visible unit; the board runs 120 Hz, where the same 16.7 ms is two frames of slack.
- **Tap to first changed frame is 50 ms, not 100.** Perception studies put the noticing threshold
  near 60 ms for a pointer; 50 ms keeps the shell under it with the display's half frame included.
- **Transitions are 200–400 ms springs, critically damped by default.** The reference desktop's
  implicit duration is 250 ms; the reference toolkit's default spring settles in about 550 ms with no
  bounce; the mobile scale puts panels at 250–400 ms. Micro-motion stays under 200 ms.
- **Nothing in these numbers needs a higher refresh rate**: smoothness on the reference device is
  zero dropped frames at 60 Hz with the motion computed for the time each frame is shown (RFC-0110).
