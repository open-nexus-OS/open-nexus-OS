# calculator — DSL app

The system calculator, built to the design artifacts (calculator): decimals with the locale's
separators, `%`, `±`, the two-stage `AC`/`C`, an expression line above the number, and an error
state on division by zero. Its window is never smaller than the artifact's phone-sized
340 × 600 and grows like the reference desktop calculator: the keys stay circles at the minimum
and become pills as the window widens.

## Routes

One page, no route table: `CalculatorPage` (`ui/pages/CalculatorPage.nx`).

## Stores and events

| Store (file) | State | Events |
|---|---|---|
| `CalcStore` (`ui/composables/calc.store.nx`) | the display (`disp`), the typed entry and its exact mantissa, the accumulator, the pending operator and its key glyph, the expression line (`history`), the two-stage clear flag (`dirty`), the error flag | `Digit(Fx, Str)`, `Sep(Str)`, `Op(Str, Str)`, `Eq`, `Clear`, `Neg`, `Percent` |
| `CalcMenuStore` (`ui/composables/menu.store.nx`) | whether the last copy landed (`copied`) | `ClearAll` (the two-stage `Clear` dispatched twice), `CopyResult` (the display's text to the clipboard), `Copied(Bool)`, `CopyFailed(Int)` — both menu actions close the menu |
| `WindowStore` (window-kit's `ui/composables/window.store.nx`, compiled in with the chrome) | the window kit's chrome state: the open menu, the panes | the window kit's `Win*` events (RFC-0084) |

The keypad carries the arithmetic as data: a key dispatches `Digit(7.0, "7")` — the value and
the character it shows — so nothing on the typing path depends on the locale catalog.

## Services

`svc.settings.set("window.control", …)` for the window controls (through the window kit);
`svc.clipboard.write(disp)` for the ⋮ menu's "copy result" — the manifest's
`nexus.permission.CLIPBOARD` grants the route (writing only: a read stays focus-gated in
clipboardd). The arithmetic uses none.

## Window

`Window { style: plain, mode: auto, resizable: true, width: 340, height: 600, minWidth: 340,
minHeight: 600 }` — the window manager opens it at 340 × 600 and never makes it smaller
([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md), RFC-0117).

## Structure

```
ui/pages/CalculatorPage.nx        the window: the kit's top bar (with ⋮) + the pane with display and keypad
ui/components/keypad/CalcDisplay  the expression line and the number (its style by length)
ui/components/keypad/CalcKeypad   the 4 × 5 keypad, each key's tap → its event (the decimal mark by region)
ui/components/keypad/CalcKey      one key: its look by kind (digit, clear, function, operator, equals)
ui/components/menu/CalcMenu       the ⋮ menu: clear all, copy the result
ui/composables/calc.store.nx      the arithmetic
ui/composables/menu.store.nx      the ⋮ menu's actions
i18n/de.json, en.json             the copy and the number formats (`calc.value`, `calc.hist.*`)
```

## Tests

In `tests/dsl_apps_conformance/tests/`: `calculator.rs` (a tap reaches the display),
`calculator_math.rs` (exact decimals, chained operators, division by zero, `%` and `±`),
`calculator_locale.rs` (German grouping and separators), `calculator_layout.rs` (the keypad
divides the window), `calculator_pipeline.rs` (tap → store → scene → layout → glyphs),
`calculator_pixels.rs` (no animation rests the display invisible), `calculator_menu.rs` (the ⋮
menu: clear all, copy what the display shows, the menu closes), and `apps_compile.rs` (the display
carries no motion). The manifest's caps are pinned in `tools/nxb-pack/tests/repo_bundles.rs`. Run them with
`cargo test -p dsl_apps_conformance --test 'calculator*'`.

## Known limits

The entry holds 9 integer digits (the fixed-point value type), not the artifact's 12. `±` and
`%` start a new entry instead of keeping the typed one editable, and the entry is not grouped
while typing — both need a sign and grouping model the store has no room for under its line
limit (a store split, TASK-0353's follow-up). The decimal key is `,` under `de-DE` and `.` in the
other regions; negative numbers show the ASCII minus (number formatting is Rust).
