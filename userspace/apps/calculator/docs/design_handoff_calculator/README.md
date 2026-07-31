# Rechner (nexus-calc)

Rechner-App im **open nexus OS Design System** (AIVA Computers). Fensterchrome = `Window`-Komponente des Design Systems, dark theme, 392 × 616 px.

## Inhalt des Pakets

```
Rechner.dc.html                  App (Design Component, direkt im Browser öffnenbar)
support.js                       Runtime für die Design Component
assets/icons/calculator.svg      App-Icon (Titelleiste)
_ds/open-nexus-os-design-system-…/
  styles.css                     Globaler CSS-Einstiegspunkt (nur @imports)
  _ds_bundle.js                  Komponenten-Bundle (Window, Icon, …)
  tokens/colors.css              Farben (light + .dark)
  tokens/typography.css          Font-Stack, Größen, Gewichte
  tokens/spacing.css             Spacing, Radius, Shadow, z-index
  tokens/glass.css               Liquid-Glass-Tokens
  tokens/motion.css              Easing-Kurven, Dauern, Keyframes
  tokens/fonts.css               Inter + Noto Sans (Google Fonts)
screenshots/
  01-standard.png                Grundzustand
  02-ergebnis.png                Rechnung mit Ergebnis (Verlaufszeile)
  03-app-menu.png                App-Menü in der Titelleiste
```

Öffnen: `Rechner.dc.html` im Browser (relative Pfade bleiben erhalten, Ordnerstruktur nicht verändern). Fonts werden von Google Fonts geladen — offline greift der System-Stack.

## Funktion

- Grundrechenarten `+ − × ÷`, `%`, `±`, Dezimaltrenner, `=`
- Zweizeiliges Display: kleine Verlaufszeile (`7 × 8 =`) über dem Ergebnis; Schriftgröße skaliert automatisch (52 → 38 → 30 px)
- `AC` / `C`: erster Druck löscht die Eingabe, zweiter den gesamten Zustand
- Max. 12 Stellen Eingabe, Rundung auf 10 Dezimalstellen, `Fehler` bei Division durch 0
- Tastatur: `0–9`, `,` `.`, `+ - * /`, `%`, `Enter`/`=`, `Backspace`, `Esc`
- App-Menü (Klick auf Icon in der Titelleiste): „Alles löschen“, „Ergebnis kopieren“

## Tweaks (Props)

| Prop | Editor | Default | Wirkung |
|---|---|---|---|
| `accent` | color | `#3b82f6` | Akzentfarbe der Operator- und `=`-Taste (Alternativen `#14b8a6`, `#8b5cf6`) |
| `decimalComma` | boolean | `true` | Dezimalkomma (de-DE) statt Punkt |
| `groupThousands` | boolean | `true` | Tausenderpunkte im Display |

## Design-Anwendung

- Display & Zifferntasten: `--glass-card-bg` / `--glass-card-border`, `backdrop-filter: blur(20px)`, Radius `--radius-xl`
- Funktionstasten (AC, ±, %): `rgba(255,255,255,0.16)`
- Operatoren: `color-mix(… var(--calc-accent) 24%)`, Hover 44 %; `=` bei 82 % mit weißem Text
- Inneres Pane: `--glass-window-pane-bg/-border/-inset`, Radius `--radius-2xl`
- Press-Feedback: `transform: scale(0.94)` 0.1 s, Release über `--motion-spring-soft`
- Zahlen durchgehend `font-variant-numeric: tabular-nums`

Datei im Projekt: `Rechner.dc.html`
