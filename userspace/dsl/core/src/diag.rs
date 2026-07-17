// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Structured diagnostics: stable codes + byte spans.
//!
//! Codes are part of the tool contract (`nx dsl explain NX0405`); they never
//! get renumbered. Rendering to pretty terminal output is the CLI's job — this
//! module stays `no_std` and value-oriented.

use alloc::string::String;
use core::fmt;

/// Byte range into one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

impl Span {
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    /// Smallest span covering both.
    #[must_use]
    pub fn to(self, other: Span) -> Span {
        Span { start: self.start.min(other.start), end: self.end.max(other.end) }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// Stable diagnostic codes. Append-only; never renumber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DiagCode {
    // --- lexer (NX00xx)
    UnexpectedChar,     // NX0001
    UnterminatedString, // NX0002
    FileTooLarge,       // NX0003
    IdentTooLong,       // NX0004
    IntOverflow,        // NX0005
    // --- parser (NX01xx)
    UnexpectedToken,  // NX0101
    DuplicateProp,    // NX0103
    TrailingTokens,   // NX0104
    NestingTooDeep,   // NX0105
    EmptyMatch,       // NX0106
    InvalidRoutePath, // NX0107
    // --- resolve (NX02xx)
    UnknownName,          // NX0201
    DuplicateDefinition,  // NX0202
    ImportConflict,       // NX0203
    UnknownWidget,        // NX0204
    UnknownModifier,      // NX0205
    UnknownEvent,         // NX0206
    UnknownService,       // NX0207 (svc.<service> not in the platform surface)
    UnknownServiceMethod, // NX0208
    // --- types (NX03xx)
    TypeMismatch,    // NX0301
    WrongArity,      // NX0302
    UnknownField,    // NX0303
    NotExhaustive,   // NX0304
    UnknownEnumCase, // NX0305
    UnknownType,     // NX0306
    ConstRequired,   // NX0307
    // --- lints (NX04xx)
    MissingKey,         // NX0401
    MissingLabel,       // NX0402
    DuplicateModifier,  // NX0403
    UnboundedFor,       // NX0404
    ReducerImpure,      // NX0405
    MissingProfileElse, // NX0406 (Warning)
    UnhandledResult,    // NX0407
    DuplicateRoute,     // NX0408
    MissingTimeout,     // NX0409
    QueryShape,         // NX0410 (query outside the v1 shape contract)
    // --- lowering (NX05xx)
    LoweringUnsupported, // NX0501 (a construct outside the v0.1 lowering subset)
}

impl DiagCode {
    /// The stable wire code (`NX####`).
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            DiagCode::UnexpectedChar => "NX0001",
            DiagCode::UnterminatedString => "NX0002",
            DiagCode::FileTooLarge => "NX0003",
            DiagCode::IdentTooLong => "NX0004",
            DiagCode::IntOverflow => "NX0005",
            DiagCode::UnexpectedToken => "NX0101",
            DiagCode::DuplicateProp => "NX0103",
            DiagCode::TrailingTokens => "NX0104",
            DiagCode::NestingTooDeep => "NX0105",
            DiagCode::EmptyMatch => "NX0106",
            DiagCode::InvalidRoutePath => "NX0107",
            DiagCode::UnknownName => "NX0201",
            DiagCode::DuplicateDefinition => "NX0202",
            DiagCode::ImportConflict => "NX0203",
            DiagCode::UnknownWidget => "NX0204",
            DiagCode::UnknownModifier => "NX0205",
            DiagCode::UnknownEvent => "NX0206",
            DiagCode::UnknownService => "NX0207",
            DiagCode::UnknownServiceMethod => "NX0208",
            DiagCode::TypeMismatch => "NX0301",
            DiagCode::WrongArity => "NX0302",
            DiagCode::UnknownField => "NX0303",
            DiagCode::NotExhaustive => "NX0304",
            DiagCode::UnknownEnumCase => "NX0305",
            DiagCode::UnknownType => "NX0306",
            DiagCode::ConstRequired => "NX0307",
            DiagCode::MissingKey => "NX0401",
            DiagCode::MissingLabel => "NX0402",
            DiagCode::DuplicateModifier => "NX0403",
            DiagCode::UnboundedFor => "NX0404",
            DiagCode::ReducerImpure => "NX0405",
            DiagCode::MissingProfileElse => "NX0406",
            DiagCode::UnhandledResult => "NX0407",
            DiagCode::DuplicateRoute => "NX0408",
            DiagCode::MissingTimeout => "NX0409",
            DiagCode::QueryShape => "NX0410",
            DiagCode::LoweringUnsupported => "NX0501",
        }
    }

    /// Default severity. Warnings promote to errors under `--deny-warn`.
    ///
    /// `UnhandledResult`/`MissingTimeout` are warnings in v0.1 and become
    /// errors when the async-recipe wave lands (TASK-0077B/0078 contract).
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            DiagCode::MissingProfileElse | DiagCode::UnhandledResult | DiagCode::MissingTimeout => {
                Severity::Warning
            }
            _ => Severity::Error,
        }
    }
}

impl fmt::Display for DiagCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// One diagnostic: code + span + human message (message text is not part of
/// the stability contract; the code and span are).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: DiagCode,
    pub span: Span,
    pub message: String,
}

impl Diagnostic {
    #[must_use]
    pub fn new(code: DiagCode, span: Span, message: String) -> Self {
        Self { code, span, message }
    }

    #[must_use]
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }
}

/// Line/column (1-based) for a byte offset — for renderers.
#[must_use]
pub fn line_col(source: &str, offset: u32) -> (u32, u32) {
    let offset = (offset as usize).min(source.len());
    let mut line = 1u32;
    let mut col = 1u32;
    for (i, ch) in source.char_indices() {
        if i >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}
