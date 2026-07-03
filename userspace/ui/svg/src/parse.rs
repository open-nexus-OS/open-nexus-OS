// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::string::String as AString;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec as AVec;
use alloc::vec::Vec;
use core::str::Chars;
use hashbrown::HashMap;

use crate::elements::{
    Color, FillRule, GradientStop, GradientUnits, LineCap, LineJoin, Paint, PathCommand, PathData,
    StrokeStyle, SvgDocument, SvgElement, Transform,
};
use crate::error::{SvgError, SvgResult};
use crate::limits::{MAX_PATH_SEGMENTS, MAX_SVG_NODES};

// ---------------------------------------------------------------------------
// XML Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum XmlToken {
    // Start tag: <name attr="val">
    OpenTag { name: AString, attrs: AVec<(String, String)> },
    // Self-closing tag: <name attr="val" />
    SelfCloseTag { name: AString, attrs: AVec<(String, String)> },
    // Closing tag: </name>
    CloseTag { name: AString },
    // Text content between tags
    Text(String),
    // End of input
    Eof,
}

struct Tokenizer<'a> {
    chars: Chars<'a>,
    current: Option<char>,
    line: usize,
    col: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(input: &'a str) -> Self {
        let mut chars = input.chars();
        let current = chars.next();
        Tokenizer { chars, current, line: 1, col: 1 }
    }

    fn advance(&mut self) {
        if let Some(c) = self.current {
            if c == '\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
        }
        self.current = self.chars.next();
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.current {
            if c.is_ascii_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn error(&self, message: &str) -> SvgError {
        SvgError::XmlParse { line: self.line, col: self.col, message: message.to_string() }
    }

    fn read_until(&mut self, stop: char) -> AString {
        let mut result = AString::new();
        while let Some(c) = self.current {
            if c == stop {
                break;
            }
            result.push(c);
            self.advance();
        }
        result
    }

    fn read_name(&mut self) -> AString {
        let mut result = AString::new();
        while let Some(c) = self.current {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' {
                result.push(c);
                self.advance();
            } else {
                break;
            }
        }
        result
    }

    fn read_quoted_string(&mut self) -> SvgResult<String> {
        let quote = self.current.ok_or_else(|| self.error("unexpected end in quoted string"))?;
        if quote != '"' && quote != '\'' {
            return Err(self.error("expected quote character"));
        }
        self.advance(); // skip opening quote
        let mut result = AString::new();
        while let Some(c) = self.current {
            if c == quote {
                self.advance(); // skip closing quote
                return Ok(result);
            }
            result.push(c);
            self.advance();
        }
        Err(self.error("unterminated quoted string"))
    }

    fn next_token(&mut self) -> SvgResult<XmlToken> {
        self.skip_whitespace();

        match self.current {
            None => Ok(XmlToken::Eof),
            Some('<') => {
                self.advance();
                match self.current {
                    Some('/') => {
                        self.advance();
                        let name = self.read_name();
                        self.skip_whitespace();
                        if self.current == Some('>') {
                            self.advance();
                        }
                        Ok(XmlToken::CloseTag { name })
                    }
                    Some('?') => {
                        // Skip processing instructions
                        self.read_until('>');
                        self.advance(); // skip >
                        self.next_token()
                    }
                    Some('!') => {
                        // Skip comments and DOCTYPE
                        self.advance();
                        if self.current == Some('-') {
                            self.advance();
                            if self.current == Some('-') {
                                self.advance();
                                // Read until -->
                                loop {
                                    if self.current == Some('-') {
                                        self.advance();
                                        if self.current == Some('-') {
                                            self.advance();
                                            if self.current == Some('>') {
                                                self.advance();
                                                break;
                                            }
                                        }
                                    } else if self.current.is_none() {
                                        return Err(self.error("unterminated comment"));
                                    } else {
                                        self.advance();
                                    }
                                }
                            }
                        } else {
                            // Skip CDATA, DOCTYPE, etc.
                            self.read_until('>');
                            self.advance();
                        }
                        self.next_token()
                    }
                    _ => {
                        let name = self.read_name();
                        let mut attrs = AVec::new();
                        loop {
                            self.skip_whitespace();
                            match self.current {
                                Some('/') => {
                                    self.advance();
                                    if self.current == Some('>') {
                                        self.advance();
                                    }
                                    return Ok(XmlToken::SelfCloseTag { name, attrs });
                                }
                                Some('>') => {
                                    self.advance();
                                    return Ok(XmlToken::OpenTag { name, attrs });
                                }
                                Some(_) => {
                                    let attr_name = self.read_name();
                                    self.skip_whitespace();
                                    if self.current == Some('=') {
                                        self.advance();
                                        self.skip_whitespace();
                                        let attr_val = self.read_quoted_string()?;
                                        attrs.push((attr_name, attr_val));
                                    }
                                }
                                None => {
                                    return Err(self.error("unexpected end in tag"));
                                }
                            }
                        }
                    }
                }
            }
            Some(_) => {
                let mut text = AString::new();
                while let Some(c) = self.current {
                    if c == '<' {
                        break;
                    }
                    text.push(c);
                    self.advance();
                }
                if text.trim().is_empty() {
                    self.next_token()
                } else {
                    Ok(XmlToken::Text(text))
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SVG Element Parser
// ---------------------------------------------------------------------------

// Parse an SVG string into an `SvgDocument`.
pub fn parse_svg(input: &str) -> SvgResult<SvgDocument> {
    parse_svg_tinted(input, Color::BLACK)
}

/// Parse with a base `tint` for `currentColor`, so monochrome icons (Lucide
/// et al., `stroke="currentColor"`) are themed by the caller's token color.
pub(crate) fn parse_svg_tinted(input: &str, tint: Color) -> SvgResult<SvgDocument> {
    let mut tokenizer = Tokenizer::new(input);
    let mut node_count = 0;
    let mut segments = 0;

    // One entry per currently-open container (`<svg>`/`<g>`/`<defs>`): the children
    // accumulated so far plus the group transform/opacity and the resolved style its
    // subtree inherits. A `<g>` pushes a frame on open and, on close, becomes a
    // `Group` carrying that transform/opacity — so nested groups compose correctly and
    // the cascade flows down through them (flat icons and grouped assets alike).
    struct ParseFrame {
        children: Vec<SvgElement>,
        transform: Option<Transform>,
        opacity: f32,
        style: StyleContext,
    }

    let mut root: Option<SvgDocument> = None;
    let mut stack: AVec<ParseFrame> = AVec::new();
    let mut defs: HashMap<String, SvgElement> = HashMap::new();
    // The presentation-property cascade root; replaced by the <svg>'s own style.
    let mut root_style = StyleContext::root(tint);

    loop {
        let token = tokenizer.next_token()?;
        match token {
            XmlToken::OpenTag { name, attrs } => {
                node_count += 1;
                if node_count > MAX_SVG_NODES {
                    return Err(SvgError::TooManyNodes { count: node_count, limit: MAX_SVG_NODES });
                }

                let tag_lower = name.to_lowercase();
                check_allowed_tag(&tag_lower, tokenizer.line)?;
                check_attrs(&tag_lower, &attrs, tokenizer.line)?;

                match tag_lower.as_str() {
                    "svg" => {
                        let (w, h) = parse_dimensions(&attrs)?;
                        // The root <svg>'s presentation attrs seed the cascade.
                        root_style = resolve_context(&attrs, &StyleContext::root(tint));
                        root = Some(SvgDocument {
                            width: w,
                            height: h,
                            elements: AVec::new(),
                            defs: HashMap::new(),
                        });
                        stack.push(ParseFrame {
                            children: Vec::new(),
                            transform: None,
                            opacity: 1.0,
                            style: root_style.clone(),
                        });
                    }
                    "defs" => {
                        let style = stack
                            .last()
                            .map(|f| f.style.clone())
                            .unwrap_or_else(|| root_style.clone());
                        stack.push(ParseFrame {
                            children: Vec::new(),
                            transform: None,
                            opacity: 1.0,
                            style,
                        });
                    }
                    "g" => {
                        // A group nests: resolve its style from the parent, capture its
                        // own transform/opacity, and push a frame its children collect
                        // into. The transform is applied when the group closes.
                        let style = resolve_context(
                            &attrs,
                            stack.last().map(|f| &f.style).unwrap_or(&root_style),
                        );
                        let transform = parse_transform_attr(&attrs);
                        let opacity = parse_opacity_attr(&attrs);
                        stack.push(ParseFrame { children: Vec::new(), transform, opacity, style });
                    }
                    _ => {
                        let elem = {
                            let inherited =
                                stack.last().map(|f| &f.style).unwrap_or(&root_style);
                            parse_element(
                                &tag_lower,
                                &attrs,
                                &mut tokenizer,
                                &mut node_count,
                                &mut segments,
                                &mut defs,
                                inherited,
                            )?
                        };
                        if let Some(frame) = stack.last_mut() {
                            frame.children.push(elem);
                        }
                    }
                }
            }
            XmlToken::SelfCloseTag { name, attrs } => {
                node_count += 1;
                if node_count > MAX_SVG_NODES {
                    return Err(SvgError::TooManyNodes { count: node_count, limit: MAX_SVG_NODES });
                }

                let tag_lower = name.to_lowercase();
                check_allowed_tag(&tag_lower, tokenizer.line)?;
                check_attrs(&tag_lower, &attrs, tokenizer.line)?;

                let elem = {
                    let inherited = stack.last().map(|f| &f.style).unwrap_or(&root_style);
                    parse_element(
                        &tag_lower,
                        &attrs,
                        &mut tokenizer,
                        &mut node_count,
                        &mut segments,
                        &mut defs,
                        inherited,
                    )?
                };

                // Push self-closing element into parent's children
                if let Some(frame) = stack.last_mut() {
                    frame.children.push(elem);
                }
            }
            XmlToken::CloseTag { name } => {
                let tag_lower = name.to_lowercase();
                match tag_lower.as_str() {
                    "svg" => {
                        if let Some(frame) = stack.pop() {
                            if let Some(ref mut doc) = root {
                                doc.elements = frame.children;
                                doc.defs = defs;
                            }
                        }
                        break; // done
                    }
                    "defs" => {
                        // Defs are handled inline during parse_element
                        let _ = stack.pop();
                    }
                    "g" => {
                        if let Some(frame) = stack.pop() {
                            let group = SvgElement::Group {
                                children: frame.children,
                                transform: frame.transform,
                                opacity: frame.opacity,
                            };
                            if let Some(parent) = stack.last_mut() {
                                parent.children.push(group);
                            }
                        }
                    }
                    _ => {
                        // Leaf elements were handled inline
                    }
                }
            }
            XmlToken::Text(_) => {
                // Ignore text content (whitespace between elements)
            }
            XmlToken::Eof => break,
        }
    }

    match root {
        Some(doc) => Ok(doc),
        None => Err(SvgError::MissingRoot),
    }
}

// ---------------------------------------------------------------------------
// Allowed tags and attributes
// ---------------------------------------------------------------------------

const ALLOWED_TAGS: &[&str] = &[
    "svg",
    "g",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polygon",
    "defs",
    "lineargradient",
    "radialgradient",
    "stop",
];

const REJECTED_TAGS: &[(&str, &str)] = &[
    ("script", "scripts are rejected"),
    ("foreignobject", "foreignObject is rejected"),
    ("filter", "filters are rejected"),
    ("animate", "animations are rejected"),
    ("animatetransform", "animations are rejected"),
    ("set", "animations are rejected"),
    ("use", "external <use> references are rejected"),
];

const KNOWN_ATTRS: &[&str] = &[
    "id",
    "class",
    "style",
    "version",
    "d",
    "fill",
    "stroke",
    "color",
    "stroke-width",
    "stroke-linejoin",
    "stroke-linecap",
    "stroke-miterlimit",
    "fill-rule",
    "opacity",
    "x",
    "y",
    "width",
    "height",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x1",
    "y1",
    "x2",
    "y2",
    "fx",
    "fy",
    "points",
    "transform",
    "offset",
    "stop-color",
    "stop-opacity",
    "gradientunits",
    "gradienttransform",
    "viewbox",
    "xmlns",
];

fn check_allowed_tag(tag: &str, line: usize) -> SvgResult<()> {
    if ALLOWED_TAGS.contains(&tag) {
        return Ok(());
    }
    for (rejected, reason) in REJECTED_TAGS {
        if *rejected == tag {
            return Err(SvgError::UnsupportedElement { tag: format!("<{tag}> ({reason})"), line });
        }
    }
    Err(SvgError::UnsupportedElement { tag: tag.to_string(), line })
}

fn check_attrs(tag: &str, attrs: &[(String, String)], line: usize) -> SvgResult<()> {
    for (name, value) in attrs {
        // Allow xmlns attributes
        if name.starts_with("xmlns") {
            continue;
        }

        // Ignore namespaced metadata attributes (e.g. `xml:space`, and the
        // vendor-prefixed export metadata that vector editors emit). These carry no
        // rendering semantics, so a strict allowlist over *rendering* attributes must
        // not reject an otherwise-valid asset over editor cruft. Downstream parsing
        // still reads any meaningful namespaced attr directly if it supports one.
        if name.contains(':') {
            continue;
        }

        // Attribute names are matched case-insensitively (SVG allows camelCase
        // presentation attrs like `viewBox`/`gradientUnits`).
        if !KNOWN_ATTRS.contains(&name.to_lowercase().as_str()) {
            return Err(SvgError::UnsupportedAttribute {
                tag: tag.to_string(),
                attr: name.clone(),
                line,
            });
        }

        // Reject external references in URLs
        if value.starts_with("url(") {
            let inner = &value[4..value.len().saturating_sub(1)];
            if inner.starts_with('#') {
                // Internal reference — allowed (gradient refs)
                continue;
            }
            return Err(SvgError::ExternalReference { kind: format!("url({inner})"), line });
        }

        if value.starts_with("data:") {
            return Err(SvgError::ExternalReference { kind: "data: URI".to_string(), line });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Element parser
// ---------------------------------------------------------------------------

fn parse_element(
    tag: &str,
    attrs: &[(String, String)],
    tokenizer: &mut Tokenizer,
    _node_count: &mut usize,
    segments: &mut usize,
    defs: &mut HashMap<String, SvgElement>,
    inherited: &StyleContext,
) -> SvgResult<SvgElement> {
    let transform = parse_transform_attr(attrs);
    let opacity = parse_opacity_attr(attrs);
    // Resolve fill/stroke/stroke-width/stroke-style through the inherited cascade
    // (so e.g. a Lucide icon's children pick up the root <svg>'s stroke).
    let eff = resolve_context(attrs, inherited);
    let fill = eff.fill;
    let stroke = eff.stroke;
    let stroke_width = eff.stroke_width;
    let stroke_style = eff.stroke_style;

    match tag {
        "g" => Ok(SvgElement::Group { children: AVec::new(), transform, opacity }),
        "path" => {
            let d_str = get_attr(attrs, "d").unwrap_or("");
            let data = parse_path_data(d_str, segments)?;
            Ok(SvgElement::Path { data, fill, stroke, stroke_width, stroke_style, transform, opacity })
        }
        "rect" => {
            let x = parse_f32_attr(attrs, "x").unwrap_or(0.0);
            let y = parse_f32_attr(attrs, "y").unwrap_or(0.0);
            let width = parse_f32_attr(attrs, "width").unwrap_or(0.0);
            let height = parse_f32_attr(attrs, "height").unwrap_or(0.0);
            let rx = parse_f32_attr(attrs, "rx").unwrap_or(0.0);
            let ry = parse_f32_attr(attrs, "ry").unwrap_or(rx);
            Ok(SvgElement::Rect {
                x,
                y,
                width,
                height,
                rx,
                ry,
                fill,
                stroke,
                stroke_width,
                stroke_style,
                transform,
                opacity,
            })
        }
        "circle" => {
            let cx = parse_f32_attr(attrs, "cx").unwrap_or(0.0);
            let cy = parse_f32_attr(attrs, "cy").unwrap_or(0.0);
            let r = parse_f32_attr(attrs, "r").unwrap_or(0.0);
            Ok(SvgElement::Circle { cx, cy, r, fill, stroke, stroke_width, stroke_style, transform, opacity })
        }
        "ellipse" => {
            let cx = parse_f32_attr(attrs, "cx").unwrap_or(0.0);
            let cy = parse_f32_attr(attrs, "cy").unwrap_or(0.0);
            let rx = parse_f32_attr(attrs, "rx").unwrap_or(0.0);
            let ry = parse_f32_attr(attrs, "ry").unwrap_or(0.0);
            Ok(SvgElement::Ellipse {
                cx,
                cy,
                rx,
                ry,
                fill,
                stroke,
                stroke_width,
                stroke_style,
                transform,
                opacity,
            })
        }
        "line" => {
            let x1 = parse_f32_attr(attrs, "x1").unwrap_or(0.0);
            let y1 = parse_f32_attr(attrs, "y1").unwrap_or(0.0);
            let x2 = parse_f32_attr(attrs, "x2").unwrap_or(0.0);
            let y2 = parse_f32_attr(attrs, "y2").unwrap_or(0.0);
            Ok(SvgElement::Line { x1, y1, x2, y2, stroke, stroke_width, stroke_style, transform, opacity })
        }
        "polygon" => {
            let points_str = get_attr(attrs, "points").unwrap_or("");
            let points = parse_points(points_str)?;
            Ok(SvgElement::Polygon { points, fill, stroke, stroke_width, stroke_style, transform, opacity })
        }
        "lineargradient" => {
            // LinearGradient is a defs entry — parsed inline. Defaults are the SVG
            // initial values (a left→right horizontal axis over the unit box).
            let id_attr = get_attr(attrs, "id").unwrap_or("");
            let x1 = parse_ratio_attr(attrs, "x1", 0.0);
            let y1 = parse_ratio_attr(attrs, "y1", 0.0);
            let x2 = parse_ratio_attr(attrs, "x2", 1.0);
            let y2 = parse_ratio_attr(attrs, "y2", 0.0);
            let units = parse_gradient_units(attrs);

            let stops = collect_gradient_stops(tokenizer, "lineargradient")?;

            let grad =
                SvgElement::LinearGradient { id: id_attr.to_string(), x1, y1, x2, y2, stops, units };
            if !id_attr.is_empty() {
                defs.insert(id_attr.to_string(), grad.clone());
            }
            Ok(grad)
        }
        "radialgradient" => {
            // RadialGradient defs entry. SVG initial values: centre + radius at 50%
            // of the unit box; the focal point defaults to the centre.
            let id_attr = get_attr(attrs, "id").unwrap_or("");
            let cx = parse_ratio_attr(attrs, "cx", 0.5);
            let cy = parse_ratio_attr(attrs, "cy", 0.5);
            let r = parse_ratio_attr(attrs, "r", 0.5);
            let fx = parse_ratio_attr(attrs, "fx", cx);
            let fy = parse_ratio_attr(attrs, "fy", cy);
            let units = parse_gradient_units(attrs);

            let stops = collect_gradient_stops(tokenizer, "radialgradient")?;

            let grad = SvgElement::RadialGradient {
                id: id_attr.to_string(),
                cx,
                cy,
                r,
                fx,
                fy,
                stops,
                units,
            };
            if !id_attr.is_empty() {
                defs.insert(id_attr.to_string(), grad.clone());
            }
            Ok(grad)
        }
        "stop" => {
            // A bare <stop> (outside a gradient) — degenerate; wrap it so the
            // parser stays total. Not rendered on its own.
            Ok(SvgElement::LinearGradient {
                id: AString::new(),
                x1: 0.0,
                y1: 0.0,
                x2: 0.0,
                y2: 0.0,
                stops: vec![parse_stop(attrs)],
                units: GradientUnits::default(),
            })
        }
        _ => Err(SvgError::UnsupportedElement { tag: tag.to_string(), line: tokenizer.line }),
    }
}

// ---------------------------------------------------------------------------
// Attribute helpers
// ---------------------------------------------------------------------------

fn get_attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
}

fn parse_f32_attr(attrs: &[(String, String)], name: &str) -> Option<f32> {
    get_attr(attrs, name).and_then(|v| v.parse::<f32>().ok())
}

/// Case-insensitive attribute lookup — for camelCase presentation attributes
/// like `gradientUnits` that authors may write in varying case.
fn get_attr_ci<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Parse a gradient ratio value: a plain number, or a percentage (`50%` → 0.5).
fn parse_ratio(s: &str) -> Option<f32> {
    let s = s.trim();
    if let Some(p) = s.strip_suffix('%') {
        p.trim().parse::<f32>().ok().map(|v| v / 100.0)
    } else {
        s.parse::<f32>().ok()
    }
}

fn parse_ratio_attr(attrs: &[(String, String)], name: &str, default: f32) -> f32 {
    get_attr(attrs, name).and_then(parse_ratio).unwrap_or(default)
}

/// `gradientUnits`, defaulting to the SVG initial value `objectBoundingBox`.
fn parse_gradient_units(attrs: &[(String, String)]) -> GradientUnits {
    match get_attr_ci(attrs, "gradientUnits") {
        Some(v) if v.trim().eq_ignore_ascii_case("userSpaceOnUse") => GradientUnits::UserSpaceOnUse,
        _ => GradientUnits::ObjectBoundingBox,
    }
}

/// Parse one `<stop>` (offset/colour/opacity), tolerating percentage offsets.
fn parse_stop(attrs: &[(String, String)]) -> GradientStop {
    let offset = get_attr(attrs, "offset").and_then(parse_ratio).unwrap_or(0.0).clamp(0.0, 1.0);
    let color_str = get_attr(attrs, "stop-color").unwrap_or("#000");
    let color = parse_color(color_str);
    let stop_opacity =
        get_attr(attrs, "stop-opacity").and_then(parse_ratio).unwrap_or(1.0).clamp(0.0, 1.0);
    let mut c = color;
    c.a = (color.a as f32 * stop_opacity) as u8;
    GradientStop { offset, color: c }
}

/// Consume `<stop>` children until the gradient's `close_tag`, returning the
/// collected stops. Shared by `<linearGradient>` and `<radialGradient>`.
fn collect_gradient_stops(
    tokenizer: &mut Tokenizer,
    close_tag: &str,
) -> SvgResult<AVec<GradientStop>> {
    let mut stops = AVec::new();
    loop {
        match tokenizer.next_token()? {
            XmlToken::OpenTag { name, attrs } => {
                if name.to_lowercase() == "stop" {
                    stops.push(parse_stop(&attrs));
                } else {
                    return Err(SvgError::UnsupportedElement { tag: name, line: tokenizer.line });
                }
            }
            XmlToken::SelfCloseTag { name, attrs } => {
                if name.to_lowercase() == "stop" {
                    stops.push(parse_stop(&attrs));
                }
            }
            XmlToken::CloseTag { name } => {
                if name.to_lowercase() == close_tag {
                    break;
                }
            }
            XmlToken::Eof => break,
            _ => continue,
        }
    }
    Ok(stops)
}

fn parse_opacity_attr(attrs: &[(String, String)]) -> f32 {
    parse_f32_attr(attrs, "opacity").unwrap_or(1.0).clamp(0.0, 1.0)
}

/// Parse stroke styling (`stroke-linejoin`/`stroke-linecap`/`stroke-miterlimit`),
/// per attribute falling back to the inherited `parent` value (the SVG cascade).
fn parse_stroke_style(attrs: &[(String, String)], parent: StrokeStyle) -> StrokeStyle {
    let line_join = match get_attr(attrs, "stroke-linejoin") {
        Some("round") => LineJoin::Round,
        Some("bevel") => LineJoin::Bevel,
        Some("miter") => LineJoin::Miter,
        _ => parent.line_join,
    };
    let line_cap = match get_attr(attrs, "stroke-linecap") {
        Some("round") => LineCap::Round,
        Some("square") => LineCap::Square,
        Some("butt") => LineCap::Butt,
        _ => parent.line_cap,
    };
    let miter_limit = parse_f32_attr(attrs, "stroke-miterlimit").unwrap_or(parent.miter_limit);
    StrokeStyle { line_join, line_cap, miter_limit }
}

/// Inherited presentation properties — the SVG style cascade. Captured from the
/// root `<svg>` and applied to descendants that don't specify their own.
#[derive(Clone)]
struct StyleContext {
    fill: Option<Paint>,
    stroke: Option<Paint>,
    stroke_width: f32,
    stroke_style: StrokeStyle,
    /// Value of the `color` property — what `currentColor` resolves to.
    color: Color,
}

impl StyleContext {
    /// Root of the cascade: SVG initial values, with `tint` as the base
    /// `currentColor` (so external callers can theme monochrome icons).
    fn root(tint: Color) -> Self {
        StyleContext {
            fill: None,
            stroke: None,
            stroke_width: 1.0,
            stroke_style: StrokeStyle::default(),
            color: tint,
        }
    }
}

/// Resolve an element's effective inherited style: its own presentation
/// attributes override `parent`, otherwise the parent value cascades down.
fn resolve_context(attrs: &[(String, String)], parent: &StyleContext) -> StyleContext {
    let color = get_attr(attrs, "color").map(parse_color).unwrap_or(parent.color);
    StyleContext {
        fill: parse_paint_attr(attrs, "fill", color).or_else(|| parent.fill.clone()),
        stroke: parse_paint_attr(attrs, "stroke", color).or_else(|| parent.stroke.clone()),
        stroke_width: parse_f32_attr(attrs, "stroke-width").unwrap_or(parent.stroke_width),
        stroke_style: parse_stroke_style(attrs, parent.stroke_style),
        color,
    }
}

fn parse_dimensions(attrs: &[(String, String)]) -> SvgResult<(f32, f32)> {
    // The `viewBox` ("min-x min-y width height") defines the user coordinate space the
    // geometry is authored in, so it is the reference the asset pipeline scales onto the
    // render target — it takes precedence over width/height, which exported assets often
    // set to "100%" (a viewport hint, not the coordinate extent). The extent is NOT an
    // allocation (that is bounded by the explicit output size in `rasterize_document_at`),
    // so a large viewBox rendered small is fine and is not clamped here.
    if let Some(vb) = get_attr_ci(attrs, "viewBox") {
        let mut it = vb
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty());
        let (_minx, _miny, vw, vh) = (it.next(), it.next(), it.next(), it.next());
        if let (Some(vw), Some(vh)) = (
            vw.and_then(|s| s.parse::<f32>().ok()),
            vh.and_then(|s| s.parse::<f32>().ok()),
        ) {
            if vw > 0.0 && vh > 0.0 {
                return Ok((vw, vh));
            }
        }
    }

    // Fall back to explicit width/height (absolute units), then a 100×100 default.
    let w = parse_f32_attr(attrs, "width").unwrap_or(100.0);
    let h = parse_f32_attr(attrs, "height").unwrap_or(100.0);
    Ok((w, h))
}

fn parse_paint_attr(attrs: &[(String, String)], name: &str, current_color: Color) -> Option<Paint> {
    let val = get_attr(attrs, name)?;
    if val.eq_ignore_ascii_case("none") {
        return Some(Paint::None);
    }
    // `currentColor` resolves to the inherited `color` property (icon tinting).
    if val.eq_ignore_ascii_case("currentColor") {
        return Some(Paint::Color(current_color));
    }
    if val.starts_with("url(") {
        let inner = val.trim_start_matches("url(").trim_end_matches(')');
        if let Some(id) = inner.strip_prefix('#') {
            return Some(Paint::GradientRef(id.to_string()));
        }
        return Some(Paint::None);
    }
    Some(Paint::Color(parse_color(val)))
}

fn parse_color(hex: &str) -> Color {
    let hex = hex.trim_start_matches('#');
    if hex.len() == 6 {
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
        Color { r, g, b, a: 255 }
    } else if hex.len() == 3 {
        let r = u8::from_str_radix(&hex[0..1], 16).unwrap_or(0) * 17;
        let g = u8::from_str_radix(&hex[1..2], 16).unwrap_or(0) * 17;
        let b = u8::from_str_radix(&hex[2..3], 16).unwrap_or(0) * 17;
        Color { r, g, b, a: 255 }
    } else {
        Color::BLACK
    }
}

fn parse_transform_attr(attrs: &[(String, String)]) -> Option<Transform> {
    let val = get_attr(attrs, "transform")?;
    let mut t = Transform::IDENTITY;

    let val = val.trim();
    if val.starts_with("translate(") {
        let args = val.trim_start_matches("translate(").trim_end_matches(')');
        let parts: AVec<f32> = args
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() >= 2 {
            t = Transform::translate(parts[0], parts[1]);
        } else if parts.len() == 1 {
            t = Transform::translate(parts[0], 0.0);
        }
    } else if val.starts_with("scale(") {
        let args = val.trim_start_matches("scale(").trim_end_matches(')');
        let parts: AVec<f32> = args
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() >= 2 {
            t = Transform::scale(parts[0], parts[1]);
        } else if parts.len() == 1 {
            t = Transform::scale(parts[0], parts[0]);
        }
    } else if val.starts_with("rotate(") {
        let args = val.trim_start_matches("rotate(").trim_end_matches(')');
        let angle: f32 = args
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .next()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0.0);
        t = Transform::rotate(angle);
    } else if val.starts_with("matrix(") {
        let args = val.trim_start_matches("matrix(").trim_end_matches(')');
        let parts: AVec<f32> = args
            .split(|c: char| c == ',' || c.is_ascii_whitespace())
            .filter_map(|s| s.parse::<f32>().ok())
            .collect();
        if parts.len() >= 6 {
            t = Transform {
                a: parts[0],
                b: parts[1],
                c: parts[2],
                d: parts[3],
                e: parts[4],
                f: parts[5],
            };
        }
    }

    Some(t)
}

// ---------------------------------------------------------------------------
// Path data parser
// ---------------------------------------------------------------------------

fn parse_path_data(d_str: &str, segments: &mut usize) -> SvgResult<PathData> {
    let mut commands = AVec::new();
    let mut chars = d_str.chars().peekable();
    let fill_rule = FillRule::NonZero;

    // Track the last control point for smooth curves
    let mut _last_cx: f32 = 0.0;
    let mut _last_cy: f32 = 0.0;
    let mut _last_qx: f32 = 0.0;
    let mut _last_qy: f32 = 0.0;

    while let Some(&c) = chars.peek() {
        // Skip whitespace and commas
        if c.is_ascii_whitespace() || c == ',' {
            chars.next();
            continue;
        }

        let cmd = c;
        chars.next();

        match cmd {
            'M' => {
                let (x, y) = parse_two_floats(&mut chars);
                commands.push(PathCommand::MoveTo { x, y });
                // Implicit line-to for subsequent coordinates
                while let Some((dx, dy)) = try_parse_two_floats(&mut chars) {
                    commands.push(PathCommand::LineTo { x: dx, y: dy });
                }
                _last_cx = x;
                _last_cy = y;
            }
            'm' => {
                let (dx, dy) = parse_two_floats(&mut chars);
                commands.push(PathCommand::MoveToRel { dx, dy });
                while let Some((ddx, ddy)) = try_parse_two_floats(&mut chars) {
                    commands.push(PathCommand::LineToRel { dx: ddx, dy: ddy });
                }
            }
            'L' => {
                let (x, y) = parse_two_floats(&mut chars);
                commands.push(PathCommand::LineTo { x, y });
                while let Some((dx, dy)) = try_parse_two_floats(&mut chars) {
                    commands.push(PathCommand::LineTo { x: dx, y: dy });
                }
            }
            'l' => {
                let (dx, dy) = parse_two_floats(&mut chars);
                commands.push(PathCommand::LineToRel { dx, dy });
                while let Some((ddx, ddy)) = try_parse_two_floats(&mut chars) {
                    commands.push(PathCommand::LineToRel { dx: ddx, dy: ddy });
                }
            }
            'H' => {
                let x = parse_float(&mut chars);
                commands.push(PathCommand::HorizontalTo { x });
                while let Some(fx) = try_parse_float(&mut chars) {
                    commands.push(PathCommand::HorizontalTo { x: fx });
                }
            }
            'h' => {
                let dx = parse_float(&mut chars);
                commands.push(PathCommand::HorizontalToRel { dx });
                while let Some(fdx) = try_parse_float(&mut chars) {
                    commands.push(PathCommand::HorizontalToRel { dx: fdx });
                }
            }
            'V' => {
                let y = parse_float(&mut chars);
                commands.push(PathCommand::VerticalTo { y });
                while let Some(fy) = try_parse_float(&mut chars) {
                    commands.push(PathCommand::VerticalTo { y: fy });
                }
            }
            'v' => {
                let dy = parse_float(&mut chars);
                commands.push(PathCommand::VerticalToRel { dy });
                while let Some(fdy) = try_parse_float(&mut chars) {
                    commands.push(PathCommand::VerticalToRel { dy: fdy });
                }
            }
            'C' => {
                let (x1, y1) = parse_two_floats(&mut chars);
                let (x2, y2) = parse_two_floats(&mut chars);
                let (x, y) = parse_two_floats(&mut chars);
                commands.push(PathCommand::CubicTo { x1, y1, x2, y2, x, y });
                _last_cx = x2;
                _last_cy = y2;
            }
            'c' => {
                let (dx1, dy1) = parse_two_floats(&mut chars);
                let (dx2, dy2) = parse_two_floats(&mut chars);
                let (dx, dy) = parse_two_floats(&mut chars);
                commands.push(PathCommand::CubicToRel { dx1, dy1, dx2, dy2, dx, dy });
            }
            'S' => {
                let (x2, y2) = parse_two_floats(&mut chars);
                let (x, y) = parse_two_floats(&mut chars);
                commands.push(PathCommand::SmoothCubicTo { x2, y2, x, y });
                _last_cx = x2;
                _last_cy = y2;
            }
            's' => {
                let (dx2, dy2) = parse_two_floats(&mut chars);
                let (dx, dy) = parse_two_floats(&mut chars);
                commands.push(PathCommand::SmoothCubicToRel { dx2, dy2, dx, dy });
            }
            'Q' => {
                let (x1, y1) = parse_two_floats(&mut chars);
                let (x, y) = parse_two_floats(&mut chars);
                commands.push(PathCommand::QuadraticTo { x1, y1, x, y });
                _last_qx = x1;
                _last_qy = y1;
            }
            'q' => {
                let (dx1, dy1) = parse_two_floats(&mut chars);
                let (dx, dy) = parse_two_floats(&mut chars);
                commands.push(PathCommand::QuadraticToRel { dx1, dy1, dx, dy });
            }
            'T' => {
                let (x, y) = parse_two_floats(&mut chars);
                commands.push(PathCommand::SmoothQuadraticTo { x, y });
            }
            't' => {
                let (dx, dy) = parse_two_floats(&mut chars);
                commands.push(PathCommand::SmoothQuadraticToRel { dx, dy });
            }
            // Elliptical arc: `rx ry x-axis-rotation large-arc-flag sweep-flag x y`.
            // `try_parse_float` on rx drives the implicit-repeat loop (one set or
            // many), mirroring the other path commands.
            'A' => {
                while let Some(rx) = try_parse_float(&mut chars) {
                    let ry = parse_float(&mut chars);
                    let xrot = parse_float(&mut chars);
                    let large = parse_float(&mut chars) != 0.0;
                    let sweep = parse_float(&mut chars) != 0.0;
                    let (x, y) = parse_two_floats(&mut chars);
                    commands.push(PathCommand::ArcTo { rx, ry, xrot, large, sweep, x, y });
                    *segments += 1;
                }
            }
            'a' => {
                while let Some(rx) = try_parse_float(&mut chars) {
                    let ry = parse_float(&mut chars);
                    let xrot = parse_float(&mut chars);
                    let large = parse_float(&mut chars) != 0.0;
                    let sweep = parse_float(&mut chars) != 0.0;
                    let (dx, dy) = parse_two_floats(&mut chars);
                    commands.push(PathCommand::ArcToRel { rx, ry, xrot, large, sweep, dx, dy });
                    *segments += 1;
                }
            }
            'Z' | 'z' => {
                commands.push(PathCommand::ClosePath);
            }
            _ => {
                return Err(SvgError::InvalidPathCommand { cmd });
            }
        }

        *segments += 1;
        if *segments > MAX_PATH_SEGMENTS {
            return Err(SvgError::TooManySegments { count: *segments, limit: MAX_PATH_SEGMENTS });
        }
    }

    Ok(PathData { commands, fill_rule })
}

fn parse_two_floats(chars: &mut core::iter::Peekable<Chars>) -> (f32, f32) {
    let a = parse_float(chars);
    let b = parse_float(chars);
    (a, b)
}

fn try_parse_two_floats(chars: &mut core::iter::Peekable<Chars>) -> Option<(f32, f32)> {
    let a = try_parse_float(chars)?;
    let b = try_parse_float(chars)?;
    Some((a, b))
}

fn parse_float(chars: &mut core::iter::Peekable<Chars>) -> f32 {
    try_parse_float(chars).unwrap_or(0.0)
}

fn try_parse_float(chars: &mut core::iter::Peekable<Chars>) -> Option<f32> {
    // Skip whitespace and commas
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() || c == ',' {
            chars.next();
        } else {
            break;
        }
    }

    let mut buf = AString::new();
    let mut has_digit = false;

    // Optional sign
    if let Some(&c) = chars.peek() {
        if c == '+' || c == '-' {
            buf.push(c);
            chars.next();
        }
    }

    // Digits and AT MOST ONE decimal point: a second '.' starts the NEXT number
    // (SVG number grammar — compact paths write "1.099.092" for 1.099 then .092;
    // consuming both dots corrupted the whole remaining parameter stream).
    let mut has_dot = false;
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            has_digit = true;
            buf.push(c);
            chars.next();
        } else if c == '.' && !has_dot {
            has_dot = true;
            buf.push(c);
            chars.next();
        } else {
            break;
        }
    }

    // Optional exponent
    if let Some(&c) = chars.peek() {
        if c == 'e' || c == 'E' {
            buf.push(c);
            chars.next();
            if let Some(&c) = chars.peek() {
                if c == '+' || c == '-' {
                    buf.push(c);
                    chars.next();
                }
            }
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() {
                    buf.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
        }
    }

    if !has_digit {
        return None;
    }

    buf.parse::<f32>().ok()
}

// ---------------------------------------------------------------------------
// Points parser
// ---------------------------------------------------------------------------

fn parse_points(s: &str) -> SvgResult<Vec<(f32, f32)>> {
    let mut points = AVec::new();
    let mut chars = s.chars().peekable();
    while let Some((x, y)) = try_parse_two_floats(&mut chars) {
        points.push((x, y));
    }
    Ok(points)
}
