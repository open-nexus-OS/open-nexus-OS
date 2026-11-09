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
| `WindowStore` (`ui/composables/window.store.nx`) | the window kit's chrome state: the open menu, the panes | the window kit's `Win*` events (RFC-0084) |

The keypad carries the arithmetic as data: a key dispatches `Digit(7.0, "7")` — the value and
the character it shows — so nothing on the typing path depends on the locale catalog.

## Services

`svc.settings.set("window.control", …)` for the window controls (through the window kit); the
arithmetic uses none.

## Window

`Window { style: plain, mode: auto, resizable: true, width: 340, height: 600, minWidth: 340,
minHeight: 600 }` — the window manager opens it at 340 × 600 and never makes it smaller
([`docs/dev/dsl/window.md`](../../../docs/dev/dsl/window.md), RFC-0117).

## Structure

```
ui/pages/CalculatorPage.nx        the window: the kit's top bar + the pane with display and keypad
ui/components/keypad/CalcDisplay  the expression line and the number (its style by length)
ui/components/keypad/CalcKeypad   the 4 × 5 keypad, each key's tap → its event
ui/components/keypad/CalcKey      one key: its look by kind (digit, clear, function, operator, equals)
ui/composables/calc.store.nx      the arithmetic
ui/composables/window.store.nx    the window chrome state
i18n/de.json, en.json             the copy and the number formats (`calc.value`, `calc.hist.*`)
```

## Tests

In `tests/dsl_apps_conformance/tests/`: `calculator.rs` (a tap reaches the display),
`calculator_math.rs` (exact decimals, chained operators, division by zero, `%` and `±`),
`calculator_locale.rs` (German grouping and separators), `calculator_layout.rs` (the keypad
divides the window), `calculator_pipeline.rs` (tap → store → scene → layout → glyphs),
`calculator_pixels.rs` (no animation rests the display invisible), and `apps_compile.rs` (the
display carries no motion). Run them with
`cargo test -p dsl_apps_conformance --test 'calculator*'`.

## Known limits

The entry holds 9 integer digits (the fixed-point value type), not the artifact's 12. `±` and
`%` start a new entry instead of keeping the typed one editable. The decimal key is `,` in every
locale. The app menu (clear all, copy the result) needs the clipboard permission and is not
built yet.
