// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::string::String;
use core::fmt;

/// Result type alias for SVG operations.
pub type SvgResult<T> = Result<T, SvgError>;

/// Errors that can occur during SVG parsing and rasterization.
#[derive(Debug)]
pub enum SvgError {
    /// XML tokenization error (unclosed tag, invalid character, etc.).
    XmlParse { line: usize, col: usize, message: String },

    /// Unsupported SVG element encountered (script, filter, animate, etc.).
    UnsupportedElement { tag: String, line: usize },

    /// Unsupported attribute on an element.
    UnsupportedAttribute { tag: String, attr: String, line: usize },

    /// External reference detected (external URL, data: URI).
    ExternalReference { kind: String, line: usize },

    /// Missing required attribute.
    MissingAttribute { tag: String, attr: String, line: usize },

    /// Invalid numeric value in attribute.
    InvalidValue { tag: String, attr: String, value: String },

    /// SVG has no `<svg>` root element.
    MissingRoot,

    /// SVG node count exceeded limit.
    TooManyNodes { count: usize, limit: usize },

    /// SVG path segment count exceeded limit.
    TooManySegments { count: usize, limit: usize },

    /// SVG dimensions exceeded limit.
    DimensionTooLarge { width: f32, height: f32, max: f32 },

    /// Invalid path data command.
    InvalidPathCommand { cmd: char },
}

impl fmt::Display for SvgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SvgError::XmlParse { line, col, message } => {
                write!(f, "XML parse error at {line}:{col}: {message}")
            }
            SvgError::UnsupportedElement { tag, line } => {
                write!(f, "unsupported SVG element <{tag}> at line {line}")
            }
            SvgError::UnsupportedAttribute { tag, attr, line } => {
                write!(f, "unsupported attribute '{attr}' on <{tag}> at line {line}")
            }
            SvgError::ExternalReference { kind, line } => {
                write!(f, "external reference ({kind}) at line {line} is rejected")
            }
            SvgError::MissingAttribute { tag, attr, line } => {
                write!(f, "missing required attribute '{attr}' on <{tag}> at line {line}")
            }
            SvgError::InvalidValue { tag, attr, value } => {
                write!(f, "invalid value '{value}' for '{attr}' on <{tag}>")
            }
            SvgError::MissingRoot => write!(f, "missing <svg> root element"),
            SvgError::TooManyNodes { count, limit } => {
                write!(f, "too many SVG nodes ({count}, limit {limit})")
            }
            SvgError::TooManySegments { count, limit } => {
                write!(f, "too many path segments ({count}, limit {limit})")
            }
            SvgError::DimensionTooLarge { width, height, max } => {
                write!(f, "SVG dimensions {width}x{height} exceed limit {max}")
            }
            SvgError::InvalidPathCommand { cmd } => {
                write!(f, "invalid path command '{cmd}'")
            }
        }
    }
}

// std::error::Error removed for no_std
