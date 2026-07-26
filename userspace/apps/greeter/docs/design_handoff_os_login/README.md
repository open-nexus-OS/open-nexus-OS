# Handoff: open nexus OS — Login / Lock Screen

Alles, was zum Nachbauen des **Sperr-/Anmeldebildschirms** gebraucht wird: eine lauffähige HTML-Datei, die Token-Verträge als CSS-Variablen, Screenshots aller Zustände und die vollständige Variablen-Referenz.

## Inhalt des Pakets

```
OsLogin.html                        Standalone, offline lauffähig, voll interaktiv
                                    → die visuelle Wahrheit (Glass braucht echtes backdrop-filter)
reference/os-login-source.dc.html   Design-System-Quelle (Template + Logik) des Screens
reference/tokens/                   colors · fonts · glass · motion · spacing · typography
                                    → der numerische Vertrag, 1:1 übernehmen
screenshots/01-login-light.png      Light Mode, Desktop
screenshots/02-login-dark.png       Dark Mode, Desktop
screenshots/03-login-error.png      Falsches Passwort (Shake + Fehlerzeile)
screenshots/04-login-phone.png      Phone-Preset (390×844), skaliert
screenshots/05-unlocked.png         Entsperrter Zustand
README.md                           Dieses Dokument
```

> **Wichtig:** `html-to-image`-Screenshots können `backdrop-filter` nicht rendern — die Glasflächen wirken darin flacher/transparenter als real. Für die echte Optik `OsLogin.html` im Browser öffnen.

**Dev-Harness in `OsLogin.html`:** oben rechts Light/Dark und die Geräte-Presets Fill · Desktop (1440×900) · Tablet (834×1112) · Phone (390×844). Diese Leiste (`.dev`) ist **nicht Teil des Produkts** — im Build entfernen. Testpasswort: `nexus`.

---

## Aufbau (5 Ebenen, von hinten nach vorn)

| z | Ebene | Wert |
|---|---|---|
| – | Wallpaper | `object-fit:cover`, vollflächig, Fallback-BG `#0d1117` |
| – | Tint | `--nx-tint` — hält Text lesbar, ohne das Bild zu töten |
| – | Vignette | `linear-gradient(to top, --nx-vignette, transparent 30%)` — Lesbarkeit Fußzeile |
| 10 | Lock-Layer | Flex-Spalte, `justify-content:space-between`: Uhr · Login-Block · Fußzeile |
| 5 | Unlocked-Layer | nur Wallpaper + Button „Wieder sperren“ |

**Padding Lock-Layer:** `clamp(28px,6cqh,64px)` vertikal, `clamp(16px,4cqw,48px)` horizontal.

**Keine Breakpoints.** Der Screen ist **fluid** über `clamp()` / `min()`; Desktop, Tablet und Phone sind dieselbe Komposition in anderer Größe. In der Standalone-Datei sind die Einheiten `cqw`/`cqh` (Container-Query-Units), damit die Geräte-Presets korrekt skalieren; in der DC-Quelle stehen `vw`/`vh` (Vollbild). Für den Rust-Port: **relativ zur Fläche des Screens rechnen**, nicht zur Bildschirmdiagonale.

---

## Design-Variablen (Konfiguration)

Der `CONFIG`-Block am Anfang des `<script>` in `OsLogin.html` ist identisch mit den Props der Design-System-Quelle.

| Variable | Typ | Default | Bedeutung |
|---|---|---|---|
| `userNames` | string (CSV) | `Jenning Schäfer, Suna Leem, Gast` | Benutzerliste; **erster Eintrag = aktiver Benutzer**. Initialen = erste 2 Wortanfänge, uppercase |
| `correctPassword` | string | `nexus` | `''` akzeptiert jede Eingabe (Demo-Modus) |
| `showUserSwitcher` | boolean | `true` | Fußzeile links; wird bei < 2 Benutzern automatisch ausgeblendet |
| `showHint` | boolean | `true` | Zeile „Mit Enter anmelden“ unter dem Feld |
| `theme` | `light` \| `dark` | `light` | wählt Token-Set **und** Wallpaper |
| `wallpaperUrl` | string | Unsplash Bergpanorama | Light-Wallpaper |
| `darkWallpaperUrl` | string | Unsplash Platine dunkel | Dark-Wallpaper |
| `locale` | string | `de-DE` | Uhrzeit 24 h `HH:MM`, Datum `Wochentag, D. Monat` |
| `unlockDurationMs` | number | `620` | muss zu `@keyframes nx-unlock` (0.6 s) passen |

**Power-Aktionen** (`POWER`, Fußzeile rechts, in dieser Reihenfolge): Ruhezustand · Neustart · Ausschalten. Lucide-Icons, 17 px, `stroke-width:2`. Im Prototyp ohne Funktion — im OS an die echten Session-Aktionen binden.

---

## Farb-/Material-Variablen (Theme-Vertrag)

Jede Fläche des Screens liest ausschließlich diese 13 Variablen. Sie sind der komplette Theme-Vertrag — im Rust-Token-Layer genau so anlegen.

| Variable | Light | Dark | Verwendung |
|---|---|---|---|
| `--nx-tint` | `rgba(255,255,255,0.24)` | `rgba(0,0,0,0.55)` | Wallpaper-Tint |
| `--nx-vignette` | `rgba(255,255,255,0.45)` | `rgba(0,0,0,0.5)` | Vignette unten |
| `--nx-glass-bg` | `rgba(255,255,255,0.5)` | `rgba(18,18,20,0.4)` | Füllung aller Glasflächen |
| `--nx-glass-border` | `rgba(255,255,255,0.7)` | `rgba(255,255,255,0.10)` | 1 px Rand |
| `--nx-glass-highlight` | `rgba(255,255,255,0.6)` | `rgba(255,255,255,0.08)` | `inset 0 1px 0` Top-Shine + Fokusring |
| `--nx-accent-bg` | `rgba(255,255,255,0.6)` | `rgba(255,255,255,0.20)` | Submit-Button |
| `--nx-text-1` | `rgba(20,20,26,0.95)` | `rgba(255,255,255,0.95)` | Uhrzeit, Name, Eingabe |
| `--nx-text-2` | `rgba(20,20,26,0.68)` | `rgba(255,255,255,0.6)` | Datum, Hinweis, Benutzernamen |
| `--nx-icon` | `rgba(20,20,26,0.8)` | `rgba(255,255,255,0.85)` | Icon-Stroke |
| `--nx-placeholder` | `rgba(20,20,26,0.5)` | `rgba(255,255,255,0.45)` | Placeholder |
| `--nx-focus` | `rgba(20,20,26,0.5)` | `rgba(255,255,255,0.55)` | Randfarbe bei Fokus |
| `--nx-shadow` | `rgba(255,255,255,0.7)` | `rgba(0,0,0,0.4)` | weicher Textschatten |
| `--nx-shadow-strong` | `rgba(255,255,255,0.95)` | `rgba(0,0,0,0.7)` | Textschatten für kleine Labels |

**Zusätzlich fix (theme-unabhängig):** Fehlerfarbe Text `rgba(255,120,130,0.95)`, Fehlerrand `rgba(255,110,120,0.65)`, Hover-Füllung `rgba(255,255,255,0.18–0.28)`, Drop-Shadows `rgba(0,0,0,0.30–0.35)`.

> Der Login nutzt **eigene `--nx-*`-Variablen statt der globalen `--glass-*`-Tokens**, weil er als einziger Screen direkt auf dem Wallpaper sitzt (dichteres Tint, stärkere Textschatten). Werte sind aus `reference/tokens/glass.css` abgeleitet — beim Port beides nebeneinander legen.

---

## Maße & Typografie

| Element | Wert |
|---|---|
| Datum | `clamp(15px, 1.6cqw, 18px)` / 600 / `letter-spacing:0.02em` |
| Uhrzeit | `clamp(64px, 12cqw, 132px)` / **300** / `line-height:1.05` / `letter-spacing:-0.02em` / `tabular-nums` |
| Login-Block | Breite `min(92cqw, 340px)`, `gap:16px` |
| Avatar (aktiv) | `clamp(72px, 9cqw, 96px)` Kreis, Glas `blur(20px)`, Initialen `clamp(26px,3.2cqw,34px)` / 600 |
| Benutzername | `clamp(17px, 1.9cqw, 21px)` / 600 |
| Passwortfeld | Höhe **44px**, `border-radius:9999px`, Padding `0 8px 0 20px`, Glas `blur(40px)`, Input 15px |
| Submit-Button | 30 px Kreis, Icon Pfeil-rechts 17 px |
| Fehler / Hinweis | 12.5px / 500 |
| Switcher-Avatar | 44 px Kreis + Label 11px/600, `gap:14px` |
| Power-Button | 44 px Kreis, Glas `blur(40px)`, `gap:12px` |
| Fußzeile | `space-between`, `align-items:flex-end`, `flex-wrap:wrap` |

Alle Hit-Targets ≥ 44 px. Schrift **Inter** (300/400/500/600/700), Fallback Noto Sans → System-Stack.

**Glas-Rezept** (identisch für Avatar, Feld, Buttons): `backdrop-filter: blur(20px | 40px)` → Füllung `--nx-glass-bg` → 1 px Rand `--nx-glass-border` → `inset 0 1px 0 --nx-glass-highlight` → Drop-Shadow `0 8–12px 20–32px rgba(0,0,0,0.30–0.35)`.

---

## Zustände & Verhalten

**Phasen:** `locked → unlocking → unlocked` (und `unlocked → locked` per „Wieder sperren“).

| Zustand | Auslöser | Darstellung |
|---|---|---|
| **idle** | Default | Uhr tickt sekündlich, Hinweiszeile sichtbar |
| **focus** | Feld fokussiert | Rand `--nx-focus` + Fokusring `0 0 0 4px --nx-glass-highlight` |
| **error** | Passwort falsch | Feld-Shake `nx-shake` 0.5s `cubic-bezier(0.36,0.07,0.19,0.97)`, roter Rand, Feld geleert, Hinweis → Fehlerzeile |
| **unlocking** | Passwort korrekt | ganzer Lock-Layer: `opacity→0`, `scale(1.06)`, `blur(18px)`, 0.6s `cubic-bezier(0.4,0,0.2,1)` |
| **unlocked** | nach 620 ms | Lock-Layer weg, nur Wallpaper + „Wieder sperren“-Pill |
| **Benutzerwechsel** | Klick auf Switcher-Avatar | gewählter Benutzer wird aktiv, Passwort + Fehler zurückgesetzt |

Fehler wird bei jeder Eingabe sofort zurückgenommen (`input` → Fehler aus, Hinweis wieder an).

## Motion (ein Physik-Vokabular, siehe `reference/tokens/motion.css`)

| Keyframe / Transition | Kurve | Dauer |
|---|---|---|
| `nx-clock-in` (Uhr rein: y −10 px + blur 8 px) | `cubic-bezier(0.22,1,0.36,1)` | 0.7 s |
| `nx-login-in` (Block rein: y 14 px, scale 0.985, blur 6 px) | `cubic-bezier(0.34,1.4,0.5,1)` | 0.65 s, 0.12 s Delay |
| `nx-shake` | `cubic-bezier(0.36,0.07,0.19,0.97)` | 0.5 s |
| `nx-unlock` | `cubic-bezier(0.4,0,0.2,1)` | 0.6 s |
| Hover Buttons (`scale(1.05–1.08)`) | `cubic-bezier(0.34,1.4,0.5,1)` | 0.3–0.35 s |
| Press (`scale(0.9–0.95)`) | dieselbe Feder | ~0.1 s |
| Feld Rand/Shadow | `ease` | 0.25 s / 0.35 s |

`prefers-reduced-motion: reduce` setzt alle Animationen und Transitions auf ~0 ms.

## Sprache & Copy

Deutsch, `de-DE`, 24-h-Zeit, Satzbau knapp und imperativ:
`Passwort eingeben` · `Mit Enter anmelden` · `Passwort falsch — bitte erneut versuchen` · `Wieder sperren` · `Ruhezustand` · `Neustart` · `Ausschalten`.

---

## Empfohlene Portierungs-Reihenfolge

1. Die 13 `--nx-*`-Variablen als Theme-Struct anlegen (Light + Dark), aus `reference/tokens/` verifizieren.
2. Glas-Primitive bauen (Blur → Füllung → Rand → Top-Shine → Shadow) — alle Elemente sind Varianten davon. **Das ist der einzige Teil, der echtes Renderer-Engineering braucht.**
3. Ebenen-Stack: Wallpaper → Tint → Vignette.
4. Lock-Layer als 3-Zonen-Flex (Uhr / Login / Fußzeile) mit den fluiden Clamps.
5. Uhr (Sekunden-Tick, `de-DE`), dann Passwortfeld mit Fokus/Fehler/Shake.
6. Fußzeile: Benutzerwechsler und Power-Cluster.
7. Unlock-Übergang + Rückweg (`locked`), erst danach die echte Session-Authentifizierung anbinden.
