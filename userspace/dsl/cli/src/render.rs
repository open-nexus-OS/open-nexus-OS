// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! `nx-dsl render`: a host preview of an app (or one page file) as a PNG — compile, mount in a
//! theme, lay out at a display size, paint the boxes with the shared painter
//! (`nexus-scene-raster`) and the text runs with the baked atlas (`nexus-text-baked`) the way
//! app-host draws them (one line per run, cut with an ellipsis, the face centred in the style's
//! line box), over a neutral wallpaper stand-in. A development aid for design work: the
//! compositor's blur and motion are not drawn and scroll layers draw at rest.

use nexus_layout::LayoutBox;
use nexus_layout_types::{FxPx, LayoutNode, Position, Rgba8, TextStyle};
use nexus_text_baked::measure_text::BakedTextMeasure;
use nexus_theme_tokens::{BaseTokens, ColorToken, DarkTokens, SolidTokens, Tokens};

/// The themes a reduce-transparency preview wraps (`SolidTokens` borrows a `'static` theme).
static LIGHT: BaseTokens = BaseTokens;
static DARK: DarkTokens = DarkTokens;
use std::collections::HashMap;
use std::path::Path;
use std::process::ExitCode;

/// What the app's surface is, which decides its page base: the desktop draws on the wallpaper,
/// a window on its glass, a fullscreen or overlay surface on the opaque background.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Surface {
    Desktop,
    Window,
    Fullscreen,
}

pub(crate) fn cmd_render(args: &[String]) -> ExitCode {
    let flag = |name: &str| -> Option<String> {
        args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
    };
    let Some(out) = flag("-o") else {
        eprintln!("nx-dsl render: `-o OUT.png` is required");
        return ExitCode::from(2);
    };
    let dark = match flag("--theme").as_deref() {
        None | Some("light") => false,
        Some("dark") => true,
        Some(other) => {
            eprintln!("nx-dsl render: unknown theme `{other}` (light|dark)");
            return ExitCode::from(2);
        }
    };
    let size = flag("--size").unwrap_or_else(|| String::from("1280x800"));
    let Some((w, h)) = size
        .split_once('x')
        .and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?)))
    else {
        eprintln!("nx-dsl render: `--size` takes WxH, got `{size}`");
        return ExitCode::from(2);
    };
    let (w, h) = (w.clamp(16, 7680), h.clamp(16, 4320));
    let profile = flag("--profile").unwrap_or_else(|| String::from("desktop"));
    let route = flag("--route");
    // `--dispatch Case[:arg,…]` (repeatable): events to dispatch after the load effects, in order —
    // how a preview opens a panel, a sheet or a section (`--dispatch Goto:personal`).
    let dispatches: Vec<String> = args
        .iter()
        .enumerate()
        .filter(|(_, a)| *a == "--dispatch")
        .filter_map(|(i, _)| args.get(i + 1).cloned())
        .collect();
    let mut values: Vec<String> =
        ["-o", "--theme", "--size", "--profile", "--route", "--locale", "--surface"]
            .iter()
            .filter_map(|n| flag(n))
            .collect();
    values.extend(dispatches.iter().cloned());
    let files: Vec<String> =
        crate::nx_files(args).into_iter().filter(|f| !values.contains(f)).collect();
    let Some(path) = files.first() else {
        eprintln!("nx-dsl render: no input (an app directory or a .nx file)");
        return ExitCode::from(2);
    };
    // The app's German catalog when it has one (the shipped default), unless told otherwise.
    let locale = flag("--locale").or_else(|| {
        let i18n = Path::new(path).join("i18n");
        (i18n.join("de.json").exists() || i18n.join("de.nxc").exists()).then(|| String::from("de"))
    });
    let surface = match flag("--surface").as_deref() {
        Some("desktop") => Surface::Desktop,
        Some("window") => Surface::Window,
        Some("fullscreen") => Surface::Fullscreen,
        Some(other) => {
            eprintln!("nx-dsl render: unknown surface `{other}` (desktop|window|fullscreen)");
            return ExitCode::from(2);
        }
        None if path.contains("desktop-shell") || path.contains("greeter") => Surface::Desktop,
        None => Surface::Window,
    };

    let mut prepared = match crate::prepare::prepare("render", path, locale.as_deref(), &profile) {
        Ok(p) => p,
        Err(code) => return code,
    };
    prepared.device.theme = if dark { "dark" } else { "light" };
    // `device.sizeClass` follows the real surface width, as app-host derives it on the device
    // (compact < 640 ≤ regular < 1024 ≤ wide) — a window narrower than the display shows its
    // regular tier (overlay panes) here too.
    prepared.device.size_class = match w {
        0..=639 => "compact",
        640..=1023 => "regular",
        _ => "wide",
    };
    // `--solid`: the reduce-transparency theme (no blur, the glass roles solid).
    let solid = args.iter().any(|a| a == "--solid");
    let solid_tokens = SolidTokens { base: if dark { &DARK } else { &LIGHT } };
    let tokens: &dyn Tokens = match (solid, dark) {
        (true, _) => &solid_tokens,
        (false, true) => &DarkTokens,
        (false, false) => &BaseTokens,
    };
    let catalog_refs: Vec<&nexus_dsl_runtime::Catalog> = prepared.catalogs.iter().collect();
    let locale = nexus_dsl_runtime::LocaleChain::new(&catalog_refs, &prepared.key_names);
    let mut view =
        match nexus_dsl_runtime::View::mount(&prepared.nxir, tokens, &prepared.device, &locale) {
            Ok(view) => view,
            Err(e) => {
                eprintln!("nx-dsl render: view mount failed: {e:?}");
                return ExitCode::from(1);
            }
        };
    // The app's load effects run against no services: a screen shows its empty or failure state.
    if let Err(e) =
        view.run_initial_effects(tokens, &prepared.device, &locale, &mut nexus_dsl_runtime::NoIo)
    {
        eprintln!("nx-dsl render: initial effects stopped ({e:?}) — rendering what stands");
    }
    if let Some(route) = &route {
        if let Err(e) = view.navigate(tokens, &prepared.device, &locale, route) {
            eprintln!("nx-dsl render: navigate `{route}` failed: {e:?}");
            return ExitCode::from(1);
        }
    }
    for spec in &dispatches {
        if let Err(msg) = dispatch_spec(&mut view, tokens, &prepared.device, &locale, spec) {
            eprintln!("nx-dsl render: --dispatch `{spec}`: {msg}");
            return ExitCode::from(1);
        }
    }
    let layout = match nexus_layout::LayoutEngine::new().layout_with_viewport(
        view.scene(),
        FxPx::new(w as i32),
        Some(FxPx::new(h as i32)),
        &BakedTextMeasure,
    ) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("nx-dsl render: layout failed: {e:?}");
            return ExitCode::from(1);
        }
    };

    let bgra = paint(view.scene(), &layout.boxes, (w, h), tokens, dark, surface);
    let mut rgba = bgra;
    for px in rgba.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    if let Err(e) = std::fs::write(&out, crate::png::encode_rgba(w, h, &rgba)) {
        eprintln!("nx-dsl render: cannot write `{out}`: {e}");
        return ExitCode::from(2);
    }
    println!(
        "rendered {path} ({}, {w}x{h}, {} boxes) -> {out}",
        if dark { "dark" } else { "light" },
        layout.boxes.len()
    );
    ExitCode::SUCCESS
}

/// One text run: its node, content, style and BGRA colour.
#[derive(Clone)]
struct Run {
    node: usize,
    content: String,
    style: TextStyle,
    color: [u8; 4],
}

/// Pre-order text collection in the layout engine's numbering (the root is 1; a stack's
/// in-flow children before its `.overlay()` ones) — the same walk app-host paints by.
fn collect(node: &LayoutNode, index: &mut usize, out: &mut Vec<Run>) {
    *index += 1;
    match node {
        LayoutNode::Text(text, _) => {
            let c = text.style.color;
            out.push(Run {
                node: *index,
                content: String::from(text.content.as_str()),
                style: text.style.clone(),
                color: [c.b, c.g, c.r, c.a],
            });
        }
        LayoutNode::TextInput(input, _) => {
            let c = input.style.color;
            let (content, color) = match (&input.placeholder, input.content.as_str().is_empty()) {
                (Some(p), true) => {
                    // As app-host paints it: the builder's theme colour, else a third of the alpha.
                    let q = input.placeholder_color.unwrap_or(Rgba8 { a: c.a / 3, ..c });
                    (String::from(p.as_str()), [q.b, q.g, q.r, q.a])
                }
                _ => (String::from(input.content.as_str()), [c.b, c.g, c.r, c.a]),
            };
            out.push(Run { node: *index, content, style: input.style.clone(), color });
        }
        LayoutNode::Stack(_, _, children) => {
            let absolute = |c: &&LayoutNode| matches!(c.item().position, Position::Absolute);
            for child in children.iter().filter(|c| !absolute(c)) {
                collect(child, index, out);
            }
            for child in children.iter().filter(absolute) {
                collect(child, index, out);
            }
        }
        LayoutNode::Grid(_, _, children) => {
            for child in children {
                collect(child, index, out);
            }
        }
        LayoutNode::Spacer(_) => {}
    }
}

/// Paints the frame (BGRA): the wallpaper stand-in, the page base, every box, every text run.
fn paint(
    scene: &LayoutNode,
    boxes: &[LayoutBox],
    (w, h): (u32, u32),
    tokens: &dyn Tokens,
    dark: bool,
    surface: Surface,
) -> Vec<u8> {
    let (top, bottom): ([u8; 3], [u8; 3]) = if dark {
        ([0x1d, 0x2b, 0x4a], [0x3a, 0x2a, 0x4f])
    } else {
        ([0x9f, 0xb4, 0xd6], [0xd9, 0xc3, 0xe3])
    };
    let base = match surface {
        Surface::Desktop => None,
        Surface::Window => Some((tokens.color(ColorToken::GlassWindow), 168u8)),
        Surface::Fullscreen => Some((tokens.color(ColorToken::Background), 255u8)),
    };
    // The surface as app-host rasters it — premultiplied BGRA from its page base (a desktop's is
    // transparent) — and windowd's composite of it over the wallpaper (src-over). Painting
    // straight onto the wallpaper showed a glass root's reset tint over black.
    let mut buf = vec![0u8; (w * h * 4) as usize];
    if let Some((c, a)) = base {
        let pre = |v: u8| (u32::from(v) * u32::from(a) / 255) as u8;
        let px = [pre(c.b), pre(c.g), pre(c.r), a];
        for p in buf.chunks_exact_mut(4) {
            p.copy_from_slice(&px);
        }
    }
    let boxes = &clip_to_viewports(boxes);
    for y in 0..h {
        let row = &mut buf[(y * w * 4) as usize..((y + 1) * w * 4) as usize];
        let mut canvas = nexus_scene_raster::RowCanvas::new(row, y as i32, w as i32);
        nexus_scene_raster::paint_row(&mut canvas, boxes);
    }
    let mut runs = Vec::new();
    collect(scene, &mut 0, &mut runs);
    let by_node: HashMap<usize, &LayoutBox> = boxes.iter().map(|b| (b.node_id, b)).collect();
    // app-host's glyph-pass occlusion (`probe/paint.rs`): text paints after every box, so a run
    // whose box overlaps a LATER glass or opaque root (an overlay panel, a menu) is dropped
    // instead of printing over the panel.
    let roots: Vec<&LayoutBox> = boxes
        .iter()
        .filter(|g| g.visual.material.is_some_and(|m| m.blurs() || m.opaque()) && !g.glass_nested)
        .collect();
    let overlaps = |a: &LayoutBox, g: &LayoutBox| {
        a.rect.x.0 < g.rect.x.0 + g.rect.width.0
            && a.rect.x.0 + a.rect.width.0 > g.rect.x.0
            && a.rect.y.0 < g.rect.y.0 + g.rect.height.0
            && a.rect.y.0 + a.rect.height.0 > g.rect.y.0
    };
    for run in &runs {
        let Some(b) = by_node.get(&run.node) else { continue };
        if roots.iter().any(|g| g.node_id > b.node_id && overlaps(b, g)) {
            continue;
        }
        let mut run = run.clone();
        run.color[3] = (u32::from(run.color[3]) * u32::from(b.opacity) / 255) as u8;
        draw_run(&mut buf, (w, h), b, &run);
    }
    // The wallpaper stand-in (a vertical gradient) under the surface, with the shadows windowd
    // draws under the surface's regions; the compositor's blur of it is not drawn (the gradient
    // is smooth, so the glass reads the same).
    let mut wall = vec![0u8; (w * h * 3) as usize];
    for y in 0..h {
        let t = y * 255 / h.max(1);
        let lerp = |a: u8, b: u8| ((u32::from(a) * (255 - t) + u32::from(b) * t) / 255) as u8;
        let px = [lerp(top[2], bottom[2]), lerp(top[1], bottom[1]), lerp(top[0], bottom[0])];
        for p in wall[(y * w * 3) as usize..((y + 1) * w * 3) as usize].chunks_exact_mut(3) {
            p.copy_from_slice(&px);
        }
    }
    for b in boxes.iter().filter(|b| is_shadowed_region(b)) {
        region_shadow(&mut wall, (w, h), b);
    }
    for (p, wp) in buf.chunks_exact_mut(4).zip(wall.chunks_exact(3)) {
        let inv = 255 - u32::from(p[3]);
        for k in 0..3 {
            p[k] = (u32::from(p[k]) + u32::from(wp[k]) * inv / 255).min(255) as u8;
        }
        p[3] = 255;
    }
    buf
}

/// A box app-host declares as a compositor region with a shadow (a blur root, or a fully opaque
/// pane/overlay root, outside any scroll layer) — the painter leaves its shadow to windowd.
fn is_shadowed_region(b: &LayoutBox) -> bool {
    let full = b.visual.opacity.is_none_or(|o| o.0 >= 255);
    b.visual.shadow.is_some()
        && !b.glass_nested
        && b.scroll_root.is_none()
        && b.visual.material.is_some_and(|m| m.blurs() || (m.opaque() && full))
}

/// windowd's region shadow (`material_glass.rs`: blur 24, offset 8 down, alpha 80) under `b`,
/// outside its footprint: a rounded-rect distance with a linear falloff over the blur.
fn region_shadow(wall: &mut [u8], (w, h): (u32, u32), b: &LayoutBox) {
    const BLUR: f32 = 24.0;
    const OFFSET_Y: i32 = 8;
    const ALPHA: f32 = 80.0;
    let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    if bw <= 0 || bh <= 0 {
        return;
    }
    let r = (b.visual.corner_radius.top_left.0.max(0)).min(bw / 2).min(bh / 2) as f32;
    // The rounded-rect signed distance of point (px, py) to a rect centred at (cx, cy).
    let sdf = |px: f32, py: f32, cx: f32, cy: f32| {
        let dx = (px - cx).abs() - (bw as f32 / 2.0 - r);
        let dy = (py - cy).abs() - (bh as f32 / 2.0 - r);
        let outside = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt();
        outside + dx.max(dy).min(0.0) - r
    };
    let (cx, cy) = (bx as f32 + bw as f32 / 2.0, by as f32 + bh as f32 / 2.0);
    let reach = BLUR as i32;
    let (x0, x1) = ((bx - reach).max(0), (bx + bw + reach).min(w as i32));
    let (y0, y1) = ((by + OFFSET_Y - reach).max(0), (by + bh + OFFSET_Y + reach).min(h as i32));
    for y in y0..y1 {
        for x in x0..x1 {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            if sdf(px, py, cx, cy) <= 0.0 {
                continue; // the region covers its own footprint
            }
            let f = (0.5 - sdf(px, py, cx, cy + OFFSET_Y as f32) / BLUR).clamp(0.0, 1.0);
            let a = ALPHA * f / 255.0;
            if a <= 0.0 {
                continue;
            }
            let i = ((y as u32 * w + x as u32) * 3) as usize;
            for k in 0..3 {
                wall[i + k] = (f32::from(wall[i + k]) * (1.0 - a)) as u8;
            }
        }
    }
}

/// Dispatches `Case[:arg,…]`: the case is looked up across the program's event types (it must
/// name exactly one); an argument is `true`/`false`, an integer, a decimal (a fixed-point `Fx`),
/// a quoted string (`'1'`), or else a string.
fn dispatch_spec(
    view: &mut nexus_dsl_runtime::View<'_>,
    tokens: &dyn Tokens,
    device: &dyn nexus_dsl_runtime::DeviceEnv,
    locale: &dyn nexus_dsl_runtime::LocaleSource,
    spec: &str,
) -> Result<(), String> {
    use nexus_dsl_runtime::Value;
    let (case, args) = spec.split_once(':').unwrap_or((spec, ""));
    let found = view.runtime().event_cases_named(case);
    let &[(event, index)] = found.as_slice() else {
        return Err(format!("names {} event cases (needs exactly one)", found.len()));
    };
    let payload = args
        .split(',')
        .filter(|a| !a.is_empty())
        .map(|a| match a {
            "true" => Value::Bool(true),
            "false" => Value::Bool(false),
            // A quoted argument is a string whatever it looks like (`Digit:1.0,'1'`).
            _ if a.len() >= 2 && (a.starts_with('\'') && a.ends_with('\'')) => {
                Value::Str(a[1..a.len() - 1].into())
            }
            _ => match (a.parse::<i64>(), a.parse::<f64>()) {
                (Ok(i), _) => Value::Int(i),
                (_, Ok(f)) if a.contains('.') => Value::Fx((f * 4_294_967_296.0) as i64),
                _ => Value::Str(a.into()),
            },
        })
        .collect();
    view.dispatch(tokens, device, locale, &mut nexus_dsl_runtime::NoIo, event, index, payload)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The boxes of a scroll layer clipped to their container's viewport, as the compositor shows
/// the band at rest (app-host rasters them unclipped into the band; windowd cuts the viewport).
fn clip_to_viewports(boxes: &[LayoutBox]) -> Vec<LayoutBox> {
    use nexus_layout_types::Rect;
    let viewport: HashMap<usize, Rect> =
        boxes.iter().filter_map(|b| b.content_clip.map(|c| (b.node_id, c))).collect();
    let meet = |a: Rect, b: Rect| {
        let (x0, y0) = (a.x.0.max(b.x.0), a.y.0.max(b.y.0));
        let x1 = (a.x.0 + a.width.0).min(b.x.0 + b.width.0);
        let y1 = (a.y.0 + a.height.0).min(b.y.0 + b.height.0);
        Rect::new(
            FxPx::new(x0),
            FxPx::new(y0),
            FxPx::new((x1 - x0).max(0)),
            FxPx::new((y1 - y0).max(0)),
        )
    };
    boxes
        .iter()
        .map(|b| {
            let mut b = b.clone();
            if let Some(v) = b.scroll_root.and_then(|r| viewport.get(&r)) {
                b.clip_rect = Some(b.clip_rect.map_or(*v, |c| meet(c, *v)));
            }
            b
        })
        .collect()
}

/// Paints one row of a run: `(row, y, pen, clip_end)` — the row's pixels from the box's left
/// edge, the image row, the pen's x in them and how far the row may be painted.
type RowDraw<'a> = dyn Fn(&mut [u8], u32, i32, u32) + 'a;

/// One run drawn as app-host draws it: one line from the box's left edge, cut with an ellipsis at
/// its right edge, the face centred in the style's line box, inside the box's clip — behind it
/// its text shadows (the label halo), blurred, reaching past the box by their blur.
fn draw_run(buf: &mut [u8], (w, h): (u32, u32), b: &LayoutBox, run: &Run) {
    let (bx, by, bw, bh) = (b.rect.x.0, b.rect.y.0, b.rect.width.0, b.rect.height.0);
    if bw <= 0 || bh <= 0 || run.content.is_empty() {
        return;
    }
    let weight = BakedTextMeasure::weight(run.style.font_weight);
    let (font, line) = match b.text_px {
        Some(px) => {
            let f = nexus_text_baked::FontSize::nearest(px.0, weight);
            (f, nexus_text_baked::line_height(f) as i32)
        }
        None => (BakedTextMeasure::font(&run.style), BakedTextMeasure::line_box(&run.style) as i32),
    };
    let tracking = BakedTextMeasure::tracking(&run.style);
    let top = by + (line - nexus_text_baked::line_height(font) as i32) / 2;
    let (cut, ell) = nexus_text_baked::ellipsis_cut(&run.content, font, tracking, bw.max(0) as u32);
    let shown = || {
        run.content
            .chars()
            .take(cut)
            .chain(core::iter::once(nexus_text_baked::ELLIPSIS).take(usize::from(ell)))
    };
    // `.textAlign`: a run narrower than its box sits at its centre or right edge (as app-host).
    let slack = (bw - nexus_text_baked::measure(shown(), font, tracking) as i32).max(0);
    let start = bx
        + match run.style.text_align {
            nexus_layout_types::TextAlign::Left => 0,
            nexus_layout_types::TextAlign::Center => slack / 2,
            nexus_layout_types::TextAlign::Right => slack,
        };
    let clip = b.clip_rect.map(|c| (c.x.0, c.y.0, c.x.0 + c.width.0, c.y.0 + c.height.0));
    // Every row of `(x0, y0, x1, y1)` inside the box's clip and the image, from its left edge:
    // `draw(row, y, pen, clip_end)`.
    let mut rows = |(x0, y0, x1, y1): (i32, i32, i32, i32), draw: &RowDraw<'_>| {
        let (x0, y0, x1, y1) = match clip {
            Some(c) => (x0.max(c.0), y0.max(c.1), x1.min(c.2), y1.min(c.3)),
            None => (x0, y0, x1, y1),
        };
        let (x0, x1, y0, y1) = (x0.max(0), x1.min(w as i32), y0.max(0), y1.min(h as i32));
        if x1 <= x0 {
            return;
        }
        for y in y0..y1 {
            let row = &mut buf[(y as u32 * w * 4) as usize..((y as u32 + 1) * w * 4) as usize];
            draw(&mut row[x0 as usize * 4..], y as u32, start - x0, (x1 - x0) as u32);
        }
    };
    for sh in b.visual.text_shadows.iter().rev().flatten().filter(|s| s.color.a > 0) {
        let blur = u32::from(sh.blur_radius);
        let reach = nexus_text_baked::blur_reach(blur) as i32;
        let (ox, oy, c) = (i32::from(sh.offset_x), i32::from(sh.offset_y), sh.color);
        let color = [c.b, c.g, c.r, (u32::from(c.a) * u32::from(run.color[3]) / 255) as u8];
        let area = (bx - reach + ox, by - reach + oy, bx + bw + reach + ox, by + bh + reach + oy);
        rows(area, &|row, y, pen, end| {
            nexus_text_baked::draw_text_blur_row(
                row,
                y,
                top + oy,
                pen + ox,
                end,
                shown(),
                font,
                tracking,
                color,
                blur,
            );
        });
    }
    rows((bx, by, bx + bw, by + bh), &|row, y, pen, end| {
        nexus_text_baked::draw_text_row(row, y, top, pen, end, shown(), font, tracking, run.color);
    });
}
