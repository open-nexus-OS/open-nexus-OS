# Handoff: open nexus OS — Status Panels & Topbar-Trigger → Rust

Alles, was zum Nachbau dieser sechs Drop-down-Panels **und ihrer Öffner in der Topbar** gebraucht wird:
**Control Center · Mitteilungszentrale · Kalender · WLAN · Ton · Batterie/Energie.**

## Inhalt des Pakets

| Datei | Rolle |
|---|---|
| `OpenNexusPanels.html` | **Visuelle Wahrheit + lebende Spezifikation.** Offline im Browser öffnen. Teil 1: die 36px-Topbar in den Modi Desktop / Tablet / Phone — jede Pill öffnet ihr Panel am echten Anker. Teil 2: alle sechs Panels in Originalgröße nebeneinander. Alles interaktiv (Schalter, Slider, Radio-Listen, Monatsnavigation, Dark/Light). |
| `panels.css` | Das komplette Panel-Stylesheet, tokengetrieben. Klassennamen = Widget-Namen im Port. |
| `tokens/*.css` | Der **numerische Vertrag**: `colors` · `glass` · `typography` · `spacing` · `motion` · `fonts`. Werte 1:1 übernehmen. |
| `assets/icons/*.svg` | Die App-Icons der Mitteilungskarten (38×38, `border-radius:9px`). |

Das HTML ist **Referenzmaterial, kein zu portierender Code**. Layoutlogik, Tokens und Interaktionsmodell in Rust neu ausdrücken. Glas lebt von echtem GPU-`backdrop-filter` — Screenshots zeigen es nie korrekt, deshalb immer die HTML-Datei als Vorlage nehmen.

---

## 1 · Das Glas-Primitiv (zuerst bauen)

Jede Glasfläche ist immer dieselbe Schichtung — einmal als wiederverwendbares Widget bauen, alles andere legt sich darauf:

1. **Backdrop-Blur** des Hintergrunds (`blur(72px) saturate(180%)` für Panels, `blur(8px)` für Mitteilungskarten)
2. **halbtransparente Füllung** — `--glass-panel-bg`: `rgba(255,255,255,0.10)` dark / `0.50` light
3. **1px Rand** — `--glass-panel-border`: `rgba(255,255,255,0.18)` dark / `0.75` light
4. **Top-Shine** — `inset 0 1px 0 rgba(255,255,255,0.22)`
5. **Schlagschatten** — `--glass-panel-shadow`: `0 25px 50px rgba(0,0,0,0.60)` dark / `0.15` light

Drei Stufen: **Panel** (das Panel selbst) → **Card** (`--glass-card-bg` `0.08`/`0.60`, `radius:20`) → **Subtle** (Listenzeilen, `0.06`/`0.70`). Karten liegen immer *im* Panel und sind heller und weniger geblurrt als es. Das Wallpaper muss durch alle Schichten lesbar bleiben.

Hintergrund-Stack der Shell: Wallpaper → Tint (`rgba(0,0,0,0.35)` dark / `rgba(255,255,255,0.20)` light) → Shell. (Im HTML ist das Wallpaper ein CSS-Gradient, damit das Paket offline funktioniert — im Produkt ein Bild.)

---

## 2 · Topbar-Trigger (die einzigen Öffner)

Topbar: Höhe **36px**, `z-index:50`, Padding `0 12px`, drei Zonen: **links · Dynamic Island (absolut zentriert) · rechts**.
Pills: Höhe **28px**, `border-radius:9999px`, `gap:5px`, Padding `0 8px`, **kein** dauerhafter Hintergrund — nur Hover/Offen-Füllung `rgba(255,255,255,0.18)` dark / `rgba(0,0,0,0.10)` light.

Welche Pills existieren, hängt an zwei **unabhängigen** Achsen: dem expliziten Flag `desktop_mode` (der Nutzer schaltet es im Control Center) und — nur wenn es `false` ist — der Viewport-Breite.

| Modus | Bedingung | Links | Rechts |
|---|---|---|---|
| **Desktop** | `desktop_mode == true` | Zeit + `·` + Datum → **Kalender** · Mitteilungs-Pill (Glocke mit rotem Punkt, Mail-Glyph + „4", Chat-Glyph, Kalender-Glyph) → **Mitteilungen** | **vier getrennte Pills**: WLAN → WLAN-Panel · Lautsprecher → Ton-Panel · Batterie + „78%" → Batterie-Panel · Slider-Glyph → **Control Center** |
| **Tablet** | `!desktop_mode && 640 ≤ vw < 1024` | Zeit → **Kalender** · Glocke → **Mitteilungen** | **eine** Status-Pill (WLAN + Lautsprecher + Batterie + „78%") → **Control Center** |
| **Landscape** | `!desktop_mode && vw ≥ 1024` | wie Tablet | wie Tablet |
| **Phone** | `!desktop_mode && vw < 640` | Zeit **und** Glocke in **einer** Pill → **Mitteilungen** | wie Tablet |

**Merke:** Im Touch-Modus gibt es **keine** WLAN-/Ton-/Batterie-Panels — deren Inhalte klappen ins Control Center. Im Desktop-Modus gibt es umgekehrt keine kombinierte Status-Pill. Die Panels selbst sind in allen Modi identisch aufgebaut.

**Dynamic Island** — in jedem Modus exakt mittig, `z-index:60`, schwarz. Collapsed `118×28` (`radius:20`) mit Album-Chip 14×14 und vier Equalizer-Balken; bei Hover `288×64` (`radius:22`) mit Cover 44×44, Titel/Artist und Transport. Übergang `width/height 0.5s cubic-bezier(0.34,1.4,0.5,1)`, Radius `0.45s`.

**Öffnungsregeln (alle Panels):** Anker `top:44px`, `z-index:40`; links verankerte Panels `left:12px`, rechts verankerte `right:12px`. Hinter jedem Panel liegt ein unsichtbarer, bildschirmfüllender Backdrop (`z-index:39`), der bei Klick schließt. **Immer nur ein Panel gleichzeitig** — Öffnen eines Panels schließt alle anderen; erneuter Klick auf dieselbe Pill schließt.

| Panel | Breite | Anker | Entrance |
|---|---|---|---|
| Control Center | **328** | rechts | `slideUp` 0.3s `cubic-bezier(0.16,1,0.3,1)` |
| Mitteilungen | **330** | links | `slideDown` 0.35s `cubic-bezier(0.34,1.4,0.5,1)` |
| Kalender | **288** | links | `slideDown` 0.3s `cubic-bezier(0.16,1,0.3,1)` |
| WLAN / Ton / Batterie | **300** | rechts | `slideDown` 0.28s `cubic-bezier(0.16,1,0.3,1)` |

`slideUp` = `opacity 0→1`, `translateY(16px)→0`, `scale(0.97)→1`. `slideDown` = `opacity 0→1`, `translateY(-10px)→0`, `scale(0.96)→1`. Bei `prefers-reduced-motion` Dauern auf ~0 kollabieren.

---

## 3 · Control Center — 328px, `radius:30`, Padding `10px 16px 14px`

Von oben nach unten, alle Abstände `margin-bottom:10px` zwischen den Karten:

1. **Header**, rechtsbündig, `gap:4`, `margin-bottom:12`: drei 30×30 runde Icon-Buttons (Bearbeiten · Ausschalten · Einstellungen, Hover `rgba(125,125,125,0.18)`) + Avatar-Kreis 30×30 (`--avatar-bg`, Initiale 12px/700).
2. **Konnektivität** — Grid `1fr 1fr`, `gap:10`. Linke Karte: **WLAN**, **Bluetooth**. Rechte Karte: **Flugmodus**, **Ansichtsmodus**. Karten `padding:11`, innen `gap:7`.
3. **Helligkeit** — Karte `padding:13px 15px`: Sektionszeile (13px-Icon + Label 11px/600, `margin-bottom:10`), darunter Slider + runder 34px-Button **Erscheinungsbild** (`gap:10`). Der Button schaltet Dark/Light: dark → Mond auf `linear-gradient(#818cf8,#4f46e5)`, light → Sonne auf `linear-gradient(#fcd34d,#f97316)`.
4. **Ton** — gleiche Karte: Slider + **Mute**-Button. Mute aktiv = `rgba(239,68,68,0.88)`, Rand `rgba(248,113,113,0.45)`, weißes Icon.
5. **Batterie** — Karte `padding:11px 15px`: grünes Batterie-Icon + Label links, Meter (80×6, `radius:3`, Füllung `rgba(34,197,94,0.85)`) + „78%" rechts.

**Toggle-Tile** (`.tbtn`) — das zentrale Bauteil, auch in WLAN- und Batterie-Panel: volle Breite, `padding:9px 11px`, `radius:15`, `gap:9`, links Icon-Kreis 26×26, rechts zweizeiliger Text (Label 11px/600, Sub 10px). Inaktiv: `--inactive-bg` (`rgba(255,255,255,0.14)` dark / `rgba(255,255,255,0.92)` light). Aktiv: `rgba(accent,0.88)` + Rand `rgba(accent,0.45)` + Glow `0 4px 12px rgba(accent,0.30)`, Icon-Kreis `rgba(255,255,255,0.25)`, Label weiß, Sub `rgba(255,255,255,0.65)`.

Akzente: WLAN/Bluetooth `59,130,246` · Flugmodus `249,115,22` · Ansichtsmodus **Desktop** `20,184,166` (Teal) / **Tablet** `139,92,246` (Violett). Das Ansichtsmodus-Tile ist **immer aktiv** — es wechselt nur Farbe, Icon und Label. Es ist der einzige Schalter für `desktop_mode` und damit für das gesamte Shell-Layout.

**Slider** (`.slider`): Track `height:28`, `radius:14`, `inset 0 1px 2px rgba(0,0,0,0.30)`, Hintergrund `--track` (`rgba(0,0,0,0.30)` dark / `rgba(0,0,0,0.12)` light). Füllung von links, `min-width:28` (der Griff verschwindet nie ganz), dark `linear-gradient(180deg,#fff,#e8eaed)` / light `#fff`. Ein 13px-Icon liegt fix bei `left:8px` **in** der Füllung (`rgba(0,0,0,0.45)`) — es wird von der Füllung überstrichen, bewegt sich aber nicht. Wert auf 0 ziehen ⇒ Mute; Mute aufheben ⇒ Wert zurück auf 50.

Sub-Texte sind Zustandsanzeigen: WLAN `Heimnetzwerk`/`Aus`, Bluetooth & Flugmodus `Ein`/`Aus`, Kein Standby `Bildschirm bleibt an`/`Automatischer Ruhezustand`.

---

## 4 · Mitteilungszentrale — 330px, `radius:26`, Padding `14`

- Kopf: „Mitteilungen" (14px/600) links, Textbutton „Alle löschen" (11px/500, gedimmt) rechts, `margin-bottom:12`.
- **Gestapelte Mail-Karte**: dahinter eine zweite Karte, `left/right:8` eingezogen und `translateY(7px)` versetzt — sie deutet „mehrere" an, ohne Inhalt. Vordere Karte trägt das Mail-Icon mit **Zähler-Badge**: `min-width:19`, `height:19`, `radius:9.5`, `linear-gradient(180deg,#ff5b52,#ff3b30)`, Position `top:-6 right:-6`, Text 10px/700 weiß.
- **Einzelkarten**: Liste mit `gap:8`. Aufbau je Karte (`radius:18`, `padding:12`, `blur(8px)`): 38×38-App-Icon (`radius:9`) · rechts Spalte mit Kopfzeile (App-Name 11px/600 links, Relativzeit 10.5px gedimmt rechts), Titel 12px/600, Body 11.5px gedimmt, **einzeilig mit Ellipsis**.
- Hover: `filter:brightness(1.07)`.
- Relativzeit als Text („vor 5 Min.", „vor 1 Std.", „vor 2 Std."), keine absoluten Uhrzeiten.

---

## 5 · Kalender — 288px, `radius:24`, Padding `12`

- Kopf: Monat + Jahr (13px/600, `de-DE` `{month:'long', year:'numeric'}`), rechts drei 24×24-Buttons: ‹ · **Punkt** (6px, springt auf den heutigen Monat) · ›.
- **Wochenraster, Montag zuerst** (`Mo Di Mi Do Fr Sa So`, 10px/500 gedimmt). Erster Wochentag = `(weekday_of_first + 6) % 7`, davor leere Zellen.
- Zelle = Zahl-Kreis 28×28 (12px) + darunter 4px-Punkt. **Heute**: gefüllter Kreis `#3b82f6`, weiße Zahl, `font-weight:700`. **Tag mit Termin**: Punkt `#f87171`, sonst transparent (Höhe bleibt reserviert, damit das Raster nicht springt). Grid-`gap:2`.
- **„Anstehend"** (11px/500 gedimmt): maximal **3** kommende Termine, sortiert. Zeile: farbige Spine 3×32 (`radius:2`) · Titel 12px/600 · Meta 11px gedimmt = `Relativlabel · Zeit`.
- Relativlabel: `Heute` / `Morgen` / sonst `{weekday:'short', day:'numeric', month:'short'}`.
- Terminfarben: `#f87171` `#60a5fa` `#a78bfa` `#fb923c` `#34d399`.

---

## 6 · WLAN · Ton · Batterie — je 300px, `radius:30`, Padding `10px 16px 14px`

Nur im **Desktop-Modus** erreichbar. Gemeinsames Bauteil ist die **Radio-Zeile** (`.rrow`): volle Breite, `padding:9`, `radius:11`, `gap:10`; links optionales Leading-Icon (14px, Opazität 0.85), Mitte Label 12px (aktiv 600, sonst 500) + optionaler Sub 10.5px, rechts blauer Haken 14px **nur wenn aktiv**. Aktiv-Hintergrund `rgba(59,130,246,0.18)`. Zeilen sitzen in einer Karte mit `padding:6`. Gruppen-Labels über den Karten: 11px/500 gedimmt, `margin:0 4px 6px`.

**WLAN**
1. Kopf: „WLAN" (14px/600) + Glass-Toggle rechts (44×26, `radius:13`, Knopf 20px, an: `--glass-toggle-on-bg` `rgba(59,130,246,0.85)`, Knopf-Weg 18px, `0.18s cubic-bezier(0.34,1.4,0.5,1)`).
2. Karte mit Toggle-Tile **„Nur Kabel"** (Akzent Blau), Sub `Ethernet aktiv`/`WLAN erlaubt`.
3. „Verfügbare Netzwerke" — Radio-Liste. Leading = **Signalbalken** (3 Balken 3px breit, Höhen 7/10/13, inaktive Balken `opacity:0.25`) + Schloss 11px (`opacity:0.55`) bei gesicherten Netzen.
4. Ist der Master-Toggle **aus** *oder* „Nur Kabel" **an**, wird die Netzliste auf `opacity:0.4` gedimmt und ist nicht bedienbar.

**Ton**
1. Kopf „Ton". 2. „Ausgabe" — Radio-Liste mit Lautsprecher-Icon, je Zeile Gerät + Anschluss-Sub (`Integriert` / `Bluetooth` / `Extern`). 3. „Eingabe" — dieselbe Liste mit Mikrofon-Icon (`Integriert` / `USB`). 4. Karte „Lautstärke": derselbe Slider + Mute-Button wie im Control Center — **ein** gemeinsamer Lautstärke-Zustand für Control Center, Ton-Panel und Topbar.

**Batterie / Energie**
1. Kopf „Batterie" + Prozentwert rechts (13px/700). 2. Karte: grünes Batterie-Icon + Meter (`flex:1`, 6px) + Stromquelle als Text rechts (`Netzbetrieb`). 3. „Energieprofil" — Radio-Liste mit **genau drei** Optionen: `Energiesparmodus` (Blatt-Icon, „Längere Laufzeit") · `Ausbalanciert` (Waage, „Empfohlen", Default) · `Höchstleistung` (Blitz, „Maximale Leistung"). 4. Karte mit Toggle-Tile **„Kein Standby"** (Akzent Orange).

---

## 7 · Zustandsmodell für den Rust-Port

Ein einziger Shell-Zustand, den alle Panels lesen und schreiben — die Panels sind reine Ansichten darauf:

```
open_panel: Option<PanelId>   // ControlCenter | Notifications | Calendar | Wifi | Sound | Battery
desktop_mode: bool            // explizites Flag, NICHT aus der Breite abgeleitet
dark: bool
brightness: u8                // 0..100
volume: u8, muted: bool       // muted ⇔ angezeigter Wert 0
wifi_on: bool, wired_only: bool, selected_network: NetworkId
bluetooth: bool, airplane: bool
output_device: DeviceId, input_device: DeviceId
battery_percent: u8, power_source: PowerSource, power_profile: Profile, no_standby: bool
calendar_shown_month: (year, month)   // nur Anzeige; „Heute" setzt zurück
notifications: Vec<Notification>      // + mail_count für die gestapelte Karte
```

`open_panel` ist ein `Option`, kein Set — das erzwingt die Ein-Panel-Regel. Der Outside-Click-Backdrop setzt es auf `None`. `desktop_mode` und `dark` gehören zur Shell, nicht zum Control Center — das Control Center schaltet sie nur.

## 8 · Sonstiges

- **Trefferflächen** im Touch-Modus ≥ 44px. Topbar-Pills sind 28px hoch, aber die 36px-Bar zählt als Trefferfläche.
- **Sprache**: alle Texte Deutsch (`de-DE`), 24-Stunden-Zeit, Satz-Groß-/Kleinschreibung. Zeiten und Datumsangaben immer über die Locale formatieren, nie zusammenstückeln.
- **Typografie**: Inter (Fallback Noto Sans / System-Sans). Panel-Textgrößen liegen zwischen 10px und 14px — die Skala in `tokens/typography.css` und die Werte oben exakt einhalten, sonst kippt die Dichte.
- **Dark/Light**: es gibt genau zwei Themes; jeder Wert kommt aus `tokens/glass.css` bzw. `tokens/colors.css`. Keine dritten Zwischenwerte erfinden.

## Empfohlene Baureihenfolge

1. Tokens portieren (`colors`, `glass`, `typography`, `spacing`, `motion`).
2. Glas-Primitiv (Blur + Füllung + Rand + Shine + Schatten) als ein Widget, mit den drei Stufen Panel/Card/Subtle.
3. Bausteine: Toggle-Tile, Slider, Radio-Zeile, Glass-Toggle, Meter, Signalbalken, Zähler-Badge.
4. Topbar mit Pills + Anker-/Backdrop-Logik und `open_panel`.
5. Control Center (enthält den `desktop_mode`-Schalter und damit die Layout-Achse).
6. Mitteilungen, Kalender.
7. WLAN, Ton, Batterie (Desktop-only).
