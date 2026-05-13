// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::str::Chars;

use crate::elements::{
    Color, FillRule, GradientStop, Paint, PathCommand, PathData, SvgDocument, SvgElement, Transform,
};
use crate::error::{SvgError, SvgResult};
use crate::limits::{MAX_PATH_SEGMENTS, MAX_SVG_DIMENSION, MAX_SVG_NODES};

// ---------------------------------------------------------------------------
// XML Tokenizer
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum XmlToken {
    /// Start tag: <name attr="val">
    OpenTag { name: String, attrs: Vec<(String, String)> },
    /// Self-closing tag: <name attr="val" />
    SelfCloseTag { name: String, attrs: Vec<(String, String)> },
    /// Closing tag: </name>
    CloseTag { name: String },
    /// Text content between tags
    Text(String),
    /// End of input
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

    fn read_until(&mut self, stop: char) -> String {
        let mut result = String::new();
        while let Some(c) = self.current {
            if c == stop {
                break;
            }
            result.push(c);
            self.advance();
        }
        result
    }

    fn read_name(&mut self) -> String {
        let mut result = String::new();
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
        let mut result = String::new();
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
                        let mut attrs = Vec::new();
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
                let mut text = String::new();
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

/// Parse an SVG string into an `SvgDocument`.
pub fn parse_svg(input: &str) -> SvgResult<SvgDocument> {
    let mut tokenizer = Tokenizer::new(input);
    let mut node_count = 0;
    let mut segments = 0;

    let mut root: Option<SvgDocument> = None;
    let mut stack: Vec<(String, Vec<SvgElement>)> = Vec::new();
    let mut defs: HashMap<String, SvgElement> = HashMap::new();

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
                        root = Some(SvgDocument {
                            width: w,
                            height: h,
                            elements: Vec::new(),
                            defs: HashMap::new(),
                        });
                        stack.push((tag_lower, Vec::new()));
                    }
                    "defs" => {
                        stack.push((tag_lower, Vec::new()));
                    }
                    _ => {
                        let elem = parse_element(
                            &tag_lower,
                            &attrs,
                            &mut tokenizer,
                            &mut node_count,
                            &mut segments,
                            &mut defs,
                        )?;
                        if let Some((_, children)) = stack.last_mut() {
                            children.push(elem);
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

                let _elem = parse_element(
                    &tag_lower,
                    &attrs,
                    &mut tokenizer,
                    &mut node_count,
                    &mut segments,
                    &mut defs,
                )?;

                // Self-closing elements like <path ... /> have no children,
                // so we don't need a stack frame. For defs entries, parse_element
                // already handled them.
            }
            XmlToken::CloseTag { name } => {
                let tag_lower = name.to_lowercase();
                match tag_lower.as_str() {
                    "svg" => {
                        if let Some((_, children)) = stack.pop() {
                            if let Some(ref mut doc) = root {
                                doc.elements = children;
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
                        if let Some((_, children)) = stack.pop() {
                            let group =
                                SvgElement::Group { children, transform: None, opacity: 1.0 };
                            if let Some((_, parent_children)) = stack.last_mut() {
                                parent_children.push(group);
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
    "d",
    "fill",
    "stroke",
    "stroke-width",
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
    if ALLOWED_TAGS.iter().any(|&t| t == tag) {
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

        if !KNOWN_ATTRS.iter().any(|&a| a == name.as_str()) {
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
) -> SvgResult<SvgElement> {
    let transform = parse_transform_attr(attrs);
    let opacity = parse_opacity_attr(attrs);
    let fill = parse_paint_attr(attrs, "fill");
    let stroke = parse_paint_attr(attrs, "stroke");
    let stroke_width = parse_f32_attr(attrs, "stroke-width").unwrap_or(1.0);

    match tag {
        "g" => Ok(SvgElement::Group { children: Vec::new(), transform, opacity }),
        "path" => {
            let d_str = get_attr(attrs, "d").unwrap_or("");
            let data = parse_path_data(d_str, segments)?;
            Ok(SvgElement::Path { data, fill, stroke, stroke_width, transform, opacity })
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
                transform,
                opacity,
            })
        }
        "circle" => {
            let cx = parse_f32_attr(attrs, "cx").unwrap_or(0.0);
            let cy = parse_f32_attr(attrs, "cy").unwrap_or(0.0);
            let r = parse_f32_attr(attrs, "r").unwrap_or(0.0);
            Ok(SvgElement::Circle { cx, cy, r, fill, stroke, stroke_width, transform, opacity })
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
                transform,
                opacity,
            })
        }
        "line" => {
            let x1 = parse_f32_attr(attrs, "x1").unwrap_or(0.0);
            let y1 = parse_f32_attr(attrs, "y1").unwrap_or(0.0);
            let x2 = parse_f32_attr(attrs, "x2").unwrap_or(0.0);
            let y2 = parse_f32_attr(attrs, "y2").unwrap_or(0.0);
            Ok(SvgElement::Line { x1, y1, x2, y2, stroke, stroke_width, transform, opacity })
        }
        "polygon" => {
            let points_str = get_attr(attrs, "points").unwrap_or("");
            let points = parse_points(points_str)?;
            Ok(SvgElement::Polygon { points, fill, stroke, stroke_width, transform, opacity })
        }
        "lineargradient" => {
            // LinearGradient is a defs entry — parsed inline
            let id_attr = get_attr(attrs, "id").unwrap_or("");
            let x1 = parse_f32_attr(attrs, "x1").unwrap_or(0.0);
            let y1 = parse_f32_attr(attrs, "y1").unwrap_or(0.0);
            let x2 = parse_f32_attr(attrs, "x2").unwrap_or(1.0);
            let y2 = parse_f32_attr(attrs, "y2").unwrap_or(0.0);

            // Collect <stop> children
            let mut stops = Vec::new();
            loop {
                let child = tokenizer.next_token()?;
                match child {
                    XmlToken::OpenTag { name, attrs: child_attrs } => {
                        if name.to_lowercase() == "stop" {
                            let offset = parse_f32_attr(&child_attrs, "offset").unwrap_or(0.0);
                            let color_str = get_attr(&child_attrs, "stop-color").unwrap_or("#000");
                            let color = parse_color(color_str);
                            let stop_opacity =
                                parse_f32_attr(&child_attrs, "stop-opacity").unwrap_or(1.0);
                            let mut c = color;
                            c.a = (color.a as f32 * stop_opacity) as u8;
                            stops.push(GradientStop { offset, color: c });
                        } else {
                            return Err(SvgError::UnsupportedElement {
                                tag: name,
                                line: tokenizer.line,
                            });
                        }
                    }
                    XmlToken::SelfCloseTag { name, attrs: child_attrs } => {
                        if name.to_lowercase() == "stop" {
                            let offset = parse_f32_attr(&child_attrs, "offset").unwrap_or(0.0);
                            let color_str = get_attr(&child_attrs, "stop-color").unwrap_or("#000");
                            let color = parse_color(color_str);
                            let stop_opacity =
                                parse_f32_attr(&child_attrs, "stop-opacity").unwrap_or(1.0);
                            let mut c = color;
                            c.a = (color.a as f32 * stop_opacity) as u8;
                            stops.push(GradientStop { offset, color: c });
                        }
                    }
                    XmlToken::CloseTag { name } => {
                        if name.to_lowercase() == "lineargradient" {
                            break;
                        }
                    }
                    XmlToken::Eof => break,
                    _ => continue,
                }
            }

            let grad =
                SvgElement::LinearGradient { id: id_attr.to_string(), x1, y1, x2, y2, stops };
            if !id_attr.is_empty() {
                defs.insert(id_attr.to_string(), grad.clone());
            }
            Ok(grad)
        }
        "stop" => {
            let offset = parse_f32_attr(attrs, "offset").unwrap_or(0.0);
            let color_str = get_attr(attrs, "stop-color").unwrap_or("#000");
            let color = parse_color(color_str);
            Ok(SvgElement::LinearGradient {
                id: String::new(),
                x1: 0.0,
                y1: 0.0,
                x2: 0.0,
                y2: 0.0,
                stops: vec![GradientStop { offset, color }],
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

fn parse_opacity_attr(attrs: &[(String, String)]) -> f32 {
    parse_f32_attr(attrs, "opacity").unwrap_or(1.0).clamp(0.0, 1.0)
}

fn parse_dimensions(attrs: &[(String, String)]) -> SvgResult<(f32, f32)> {
    let w = parse_f32_attr(attrs, "width").unwrap_or(100.0);
    let h = parse_f32_attr(attrs, "height").unwrap_or(100.0);

    if w > MAX_SVG_DIMENSION || h > MAX_SVG_DIMENSION {
        return Err(SvgError::DimensionTooLarge { width: w, height: h, max: MAX_SVG_DIMENSION });
    }

    Ok((w, h))
}

fn parse_paint_attr(attrs: &[(String, String)], name: &str) -> Option<Paint> {
    let val = get_attr(attrs, name)?;
    if val == "none" {
        return Some(Paint::None);
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
        let parts: Vec<f32> = args
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
        let parts: Vec<f32> = args
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
        let parts: Vec<f32> = args
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
    let mut commands = Vec::new();
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

fn parse_two_floats(chars: &mut std::iter::Peekable<Chars>) -> (f32, f32) {
    let a = parse_float(chars);
    let b = parse_float(chars);
    (a, b)
}

fn try_parse_two_floats(chars: &mut std::iter::Peekable<Chars>) -> Option<(f32, f32)> {
    let a = try_parse_float(chars)?;
    let b = try_parse_float(chars)?;
    Some((a, b))
}

fn parse_float(chars: &mut std::iter::Peekable<Chars>) -> f32 {
    try_parse_float(chars).unwrap_or(0.0)
}

fn try_parse_float(chars: &mut std::iter::Peekable<Chars>) -> Option<f32> {
    // Skip whitespace and commas
    while let Some(&c) = chars.peek() {
        if c.is_ascii_whitespace() || c == ',' {
            chars.next();
        } else {
            break;
        }
    }

    let mut buf = String::new();
    let mut has_digit = false;

    // Optional sign
    if let Some(&c) = chars.peek() {
        if c == '+' || c == '-' {
            buf.push(c);
            chars.next();
        }
    }

    // Digits and decimal point
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == '.' {
            if c.is_ascii_digit() {
                has_digit = true;
            }
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
    let mut points = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some((x, y)) = try_parse_two_floats(&mut chars) {
        points.push((x, y));
    }
    Ok(points)
}
