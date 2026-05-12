# Stop Conditions — TASK-0057

## Hard stop (do not claim done without)

- [ ] Resource directory structure exists (resources/themes/, icons/, cursors/, wallpapers/, fonts/)
- [ ] Theme engine resolves tokens deterministically (base/dark/light/highcontrast)
- [ ] SVG rich subset parses and rasterizes correctly (paths, fills, strokes, gradients)
- [ ] PNG/JPG decode + scale works on host (bounded memory, reject oversized)
- [ ] HarfBuzz text shaping produces stable glyph runs (LTR+RTL)
- [ ] BreezeX cursor SVG renders to correct bitmap + hotspot
- [ ] Proof surface shows: real text + SVG cursor + SVG icon
- [ ] All host tests green (ui_v2b_host, nexus-theme, nexus-svg)
- [ ] QEMU markers: cursor svg loaded, text target visible, icon target visible
- [ ] No fake-success markers — all markers from real behavior

## Reject tests required

- [ ] test_reject_svg_script_tag
- [ ] test_reject_svg_external_reference
- [ ] test_reject_svg_filter_element
- [ ] test_reject_oversized_font
- [ ] test_reject_malformed_font_header
- [ ] test_reject_decompression_bomb_image
- [ ] test_reject_invalid_theme_toml
