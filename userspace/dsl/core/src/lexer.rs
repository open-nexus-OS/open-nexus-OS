// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Bounded lexer for `.nx` source.
//!
//! Bounds (hard requirements from TASK-0075): file size, identifier length.
//! Fractional literals lex to `Fx` (Q32.32) via pure integer math — no floats
//! anywhere in the toolchain.

use crate::diag::{DiagCode, Diagnostic, Span};
use alloc::{format, string::String, vec::Vec};

/// Maximum accepted source size (bytes).
pub const MAX_FILE_BYTES: usize = 512 * 1024;
/// Maximum identifier length (bytes).
pub const MAX_IDENT_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Ident(String),
    IntLit(i64),
    /// Raw Q32.32.
    FxLit(i64),
    StrLit(String),
    // keywords
    KwStore,
    KwEvent,
    KwReduce,
    KwPage,
    KwComponent,
    KwRoutes,
    KwWindow,
    KwProps,
    KwImport,
    KwLet,
    KwIf,
    KwElse,
    KwMatch,
    KwFor,
    KwIn,
    KwOn,
    KwTrue,
    KwFalse,
    KwDispatch,
    KwEmit,
    KwNavigate,
    KwSvc,
    KwDevice,
    KwState,
    // sigils
    DollarState,
    DollarProps,
    AtEffect,
    AtT,
    AtPersist,
    // punctuation
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Colon,
    ColonColon,
    Semi,
    Dot,
    Arrow,    // ->
    FatArrow, // =>
    Eq,
    PlusEq,
    MinusEq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    AndAnd,
    OrOr,
    Eof,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

fn keyword(ident: &str) -> Option<TokenKind> {
    Some(match ident {
        "Store" => TokenKind::KwStore,
        "Event" => TokenKind::KwEvent,
        "reduce" => TokenKind::KwReduce,
        "Page" => TokenKind::KwPage,
        "Component" => TokenKind::KwComponent,
        "Routes" => TokenKind::KwRoutes,
        "Window" => TokenKind::KwWindow,
        "props" => TokenKind::KwProps,
        "import" => TokenKind::KwImport,
        "let" => TokenKind::KwLet,
        "if" => TokenKind::KwIf,
        "else" => TokenKind::KwElse,
        "match" => TokenKind::KwMatch,
        "for" => TokenKind::KwFor,
        "in" => TokenKind::KwIn,
        "on" => TokenKind::KwOn,
        "true" => TokenKind::KwTrue,
        "false" => TokenKind::KwFalse,
        "dispatch" => TokenKind::KwDispatch,
        "emit" => TokenKind::KwEmit,
        "navigate" => TokenKind::KwNavigate,
        "svc" => TokenKind::KwSvc,
        "device" => TokenKind::KwDevice,
        "state" => TokenKind::KwState,
        _ => return None,
    })
}

/// Lexes a whole file. Fails fast with a single diagnostic.
///
/// # Errors
/// The first lexical violation, with its span.
pub fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.len() > MAX_FILE_BYTES {
        return Err(Diagnostic::new(
            DiagCode::FileTooLarge,
            Span::new(0, 0),
            format!("source exceeds {MAX_FILE_BYTES} bytes"),
        ));
    }
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0usize;

    macro_rules! push {
        ($kind:expr, $start:expr, $end:expr) => {
            tokens.push(Token { kind: $kind, span: Span::new($start as u32, $end as u32) })
        };
    }

    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if i - start > MAX_IDENT_BYTES {
                    return Err(Diagnostic::new(
                        DiagCode::IdentTooLong,
                        Span::new(start as u32, i as u32),
                        format!("identifier exceeds {MAX_IDENT_BYTES} bytes"),
                    ));
                }
                let text = &source[start..i];
                let kind = keyword(text).unwrap_or_else(|| TokenKind::Ident(String::from(text)));
                push!(kind, start, i);
            }
            b'0'..=b'9' => {
                let start = i;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let is_fx = i + 1 < bytes.len()
                    && bytes[i] == b'.'
                    && bytes[i + 1].is_ascii_digit();
                if is_fx {
                    let int_part: i64 = parse_int(&source[start..i], start, i)?;
                    i += 1; // '.'
                    let frac_start = i;
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    let raw = fx_from_parts(int_part, &source[frac_start..i]).ok_or_else(|| {
                        Diagnostic::new(
                            DiagCode::IntOverflow,
                            Span::new(start as u32, i as u32),
                            String::from("fixed-point literal out of range"),
                        )
                    })?;
                    push!(TokenKind::FxLit(raw), start, i);
                } else {
                    let value = parse_int(&source[start..i], start, i)?;
                    push!(TokenKind::IntLit(value), start, i);
                }
            }
            b'"' => {
                let start = i;
                i += 1;
                let mut value = String::new();
                loop {
                    match bytes.get(i) {
                        None | Some(b'\n') => {
                            return Err(Diagnostic::new(
                                DiagCode::UnterminatedString,
                                Span::new(start as u32, i as u32),
                                String::from("unterminated string literal"),
                            ));
                        }
                        Some(b'"') => {
                            i += 1;
                            break;
                        }
                        Some(b'\\') => {
                            let escaped = match bytes.get(i + 1) {
                                Some(b'n') => '\n',
                                Some(b't') => '\t',
                                Some(b'"') => '"',
                                Some(b'\\') => '\\',
                                _ => {
                                    return Err(Diagnostic::new(
                                        DiagCode::UnexpectedChar,
                                        Span::new(i as u32, (i + 2) as u32),
                                        String::from("unknown escape sequence"),
                                    ));
                                }
                            };
                            value.push(escaped);
                            i += 2;
                        }
                        Some(_) => {
                            // Advance by one UTF-8 char.
                            let ch_len = source[i..]
                                .chars()
                                .next()
                                .map_or(1, char::len_utf8);
                            value.push_str(&source[i..i + ch_len]);
                            i += ch_len;
                        }
                    }
                }
                push!(TokenKind::StrLit(value), start, i);
            }
            b'$' => {
                let start = i;
                i += 1;
                let word_start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                match &source[word_start..i] {
                    "state" => push!(TokenKind::DollarState, start, i),
                    "props" => push!(TokenKind::DollarProps, start, i),
                    other => {
                        return Err(Diagnostic::new(
                            DiagCode::UnexpectedChar,
                            Span::new(start as u32, i as u32),
                            format!("unknown sigil `${other}` (expected $state or $props)"),
                        ));
                    }
                }
            }
            b'@' => {
                let start = i;
                i += 1;
                let word_start = i;
                while i < bytes.len() && bytes[i].is_ascii_alphanumeric() {
                    i += 1;
                }
                match &source[word_start..i] {
                    "effect" => push!(TokenKind::AtEffect, start, i),
                    "t" => push!(TokenKind::AtT, start, i),
                    "persist" => push!(TokenKind::AtPersist, start, i),
                    other => {
                        return Err(Diagnostic::new(
                            DiagCode::UnexpectedChar,
                            Span::new(start as u32, i as u32),
                            format!("unknown attribute `@{other}`"),
                        ));
                    }
                }
            }
            _ => {
                let start = i;
                let two = if i + 1 < bytes.len() { &bytes[i..i + 2] } else { &bytes[i..i + 1] };
                let (kind, len) = match two {
                    b"->" => (TokenKind::Arrow, 2),
                    b"=>" => (TokenKind::FatArrow, 2),
                    b"::" => (TokenKind::ColonColon, 2),
                    b"==" => (TokenKind::EqEq, 2),
                    b"!=" => (TokenKind::Ne, 2),
                    b"<=" => (TokenKind::Le, 2),
                    b">=" => (TokenKind::Ge, 2),
                    b"+=" => (TokenKind::PlusEq, 2),
                    b"-=" => (TokenKind::MinusEq, 2),
                    b"&&" => (TokenKind::AndAnd, 2),
                    b"||" => (TokenKind::OrOr, 2),
                    _ => match b {
                        b'{' => (TokenKind::LBrace, 1),
                        b'}' => (TokenKind::RBrace, 1),
                        b'(' => (TokenKind::LParen, 1),
                        b')' => (TokenKind::RParen, 1),
                        b'[' => (TokenKind::LBracket, 1),
                        b']' => (TokenKind::RBracket, 1),
                        b',' => (TokenKind::Comma, 1),
                        b':' => (TokenKind::Colon, 1),
                        b';' => (TokenKind::Semi, 1),
                        b'.' => (TokenKind::Dot, 1),
                        b'=' => (TokenKind::Eq, 1),
                        b'<' => (TokenKind::Lt, 1),
                        b'>' => (TokenKind::Gt, 1),
                        b'+' => (TokenKind::Plus, 1),
                        b'-' => (TokenKind::Minus, 1),
                        b'*' => (TokenKind::Star, 1),
                        b'/' => (TokenKind::Slash, 1),
                        b'%' => (TokenKind::Percent, 1),
                        b'!' => (TokenKind::Bang, 1),
                        _ => {
                            return Err(Diagnostic::new(
                                DiagCode::UnexpectedChar,
                                Span::new(start as u32, (start + 1) as u32),
                                format!("unexpected character `{}`", &source[i..].chars().next().map(String::from).unwrap_or_default()),
                            ));
                        }
                    },
                };
                i += len;
                push!(kind, start, i);
            }
        }
    }
    push!(TokenKind::Eof, i, i);
    Ok(tokens)
}

fn parse_int(text: &str, start: usize, end: usize) -> Result<i64, Diagnostic> {
    text.parse::<i64>().map_err(|_| {
        Diagnostic::new(
            DiagCode::IntOverflow,
            Span::new(start as u32, end as u32),
            String::from("integer literal out of range"),
        )
    })
}

/// Q32.32 from integer + decimal-fraction digits, pure integer math.
/// `frac_digits` are the digits after the dot (decimal). Rounds half-up.
fn fx_from_parts(int_part: i64, frac_digits: &str) -> Option<i64> {
    if int_part < -(1i64 << 31) || int_part >= (1i64 << 31) {
        return None;
    }
    // numerator/denominator in u128 to avoid overflow (≤ 19 digits kept).
    let digits: &str = if frac_digits.len() > 18 { &frac_digits[..18] } else { frac_digits };
    let numerator: u128 = digits.parse().ok()?;
    let denominator: u128 = 10u128.checked_pow(digits.len() as u32)?;
    let frac = if denominator == 0 {
        0u128
    } else {
        ((numerator << 32) + denominator / 2) / denominator
    };
    let frac = i64::try_from(frac).ok()?;
    (int_part << 32).checked_add(frac)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_the_canonical_example_shape() {
        let src = r#"
Store UserListStore {
    users: List<User> = [],
    loading: Bool = false,
}
@effect on LoadUsers {
    let users = svc.users.list();
    dispatch(UsersLoaded(users));
}
"#;
        let tokens = lex(src).expect("lexes");
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwStore));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::AtEffect));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::KwSvc));
        assert_eq!(tokens.last().map(|t| t.kind.clone()), Some(TokenKind::Eof));
    }

    #[test]
    fn fx_literals_are_exact_q32_32() {
        let tokens = lex("1.5").expect("lexes");
        assert_eq!(tokens[0].kind, TokenKind::FxLit((1i64 << 32) + (1i64 << 31)));
        let tokens = lex("0.25").expect("lexes");
        assert_eq!(tokens[0].kind, TokenKind::FxLit(1i64 << 30));
    }

    #[test]
    fn rejects_unknown_sigils_and_unterminated_strings() {
        assert_eq!(lex("$foo").unwrap_err().code, DiagCode::UnexpectedChar);
        assert_eq!(lex("\"abc").unwrap_err().code, DiagCode::UnterminatedString);
    }

    #[test]
    fn two_char_operators_win_over_one_char() {
        let tokens = lex("a == b -> c => d :: e").expect("lexes");
        let kinds: Vec<_> = tokens.iter().map(|t| t.kind.clone()).collect();
        assert!(kinds.contains(&TokenKind::EqEq));
        assert!(kinds.contains(&TokenKind::Arrow));
        assert!(kinds.contains(&TokenKind::FatArrow));
        assert!(kinds.contains(&TokenKind::ColonColon));
    }
}
