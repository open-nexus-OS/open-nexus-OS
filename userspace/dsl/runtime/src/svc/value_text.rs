// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Canonical text codec for runtime [`Value`]s — the transcript
//! format's payload encoding. Deterministic both ways: one value ⇒ one text,
//! parse(text) round-trips, malformed text ⇒ `None` (never a guess).
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: round-trip tests in `transcript.rs`

use crate::store::Value;
use alloc::{format, string::String, vec::Vec};

/// Canonical text form: `Unit`, `Bool(true)`, `Int(-3)`, `Fx(12884901888)`,
/// `Str("…")` (with `\"`/`\\`/`\n` escapes), `List[a,b]`,
/// `Enum(2,0)[payload…]`, `Record{3:Int(1),7:Str("x")}`.
pub fn value_to_text(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Unit => out.push_str("Unit"),
        Value::Bool(b) => {
            out.push_str(if *b { "Bool(true)" } else { "Bool(false)" });
        }
        Value::Int(i) => out.push_str(&format!("Int({i})")),
        Value::Fx(raw) => out.push_str(&format!("Fx({raw})")),
        Value::Str(s) => {
            out.push_str("Str(\"");
            for ch in s.chars() {
                match ch {
                    '"' => out.push_str("\\\""),
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    _ => out.push(ch),
                }
            }
            out.push_str("\")");
        }
        Value::List(items) => {
            out.push_str("List[");
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Enum { event, case, payload } => {
            out.push_str(&format!("Enum({event},{case})["));
            for (i, item) in payload.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Record(fields) => {
            out.push_str("Record{");
            for (i, (sym, item)) in fields.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{sym}:"));
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

/// Parses the canonical text form. `None` = malformed (a transcript with a
/// bad payload fails loudly at load, not silently at replay).
#[must_use]
pub fn parse_value(text: &str) -> Option<Value> {
    let mut cursor = Cursor { text: text.as_bytes(), pos: 0 };
    let value = parse(&mut cursor)?;
    cursor.skip_ws();
    if cursor.pos == cursor.text.len() {
        Some(value)
    } else {
        None
    }
}

struct Cursor<'a> {
    text: &'a [u8],
    pos: usize,
}

impl Cursor<'_> {
    fn skip_ws(&mut self) {
        while self.text.get(self.pos) == Some(&b' ') {
            self.pos += 1;
        }
    }

    fn eat(&mut self, prefix: &str) -> bool {
        if self.text[self.pos..].starts_with(prefix.as_bytes()) {
            self.pos += prefix.len();
            true
        } else {
            false
        }
    }

    fn int_until(&mut self, terminators: &[u8]) -> Option<i64> {
        let start = self.pos;
        while self
            .text
            .get(self.pos)
            .is_some_and(|b| !terminators.contains(b))
        {
            self.pos += 1;
        }
        core::str::from_utf8(&self.text[start..self.pos]).ok()?.parse().ok()
    }
}

fn parse(cursor: &mut Cursor<'_>) -> Option<Value> {
    cursor.skip_ws();
    if cursor.eat("Unit") {
        return Some(Value::Unit);
    }
    if cursor.eat("Bool(true)") {
        return Some(Value::Bool(true));
    }
    if cursor.eat("Bool(false)") {
        return Some(Value::Bool(false));
    }
    if cursor.eat("Int(") {
        let value = cursor.int_until(b")")?;
        return cursor.eat(")").then_some(Value::Int(value));
    }
    if cursor.eat("Fx(") {
        let value = cursor.int_until(b")")?;
        return cursor.eat(")").then_some(Value::Fx(value));
    }
    if cursor.eat("Str(\"") {
        let mut out = String::new();
        loop {
            match cursor.text.get(cursor.pos)? {
                b'\\' => {
                    cursor.pos += 1;
                    match cursor.text.get(cursor.pos)? {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'n' => out.push('\n'),
                        _ => return None,
                    }
                    cursor.pos += 1;
                }
                b'"' => {
                    cursor.pos += 1;
                    return cursor.eat(")").then_some(Value::Str(out));
                }
                _ => {
                    // Advance one full UTF-8 char.
                    let rest = core::str::from_utf8(&cursor.text[cursor.pos..]).ok()?;
                    let ch = rest.chars().next()?;
                    out.push(ch);
                    cursor.pos += ch.len_utf8();
                }
            }
        }
    }
    if cursor.eat("List[") {
        let items = parse_seq(cursor, b']')?;
        return Some(Value::List(items));
    }
    if cursor.eat("Enum(") {
        let event = cursor.int_until(b",")?;
        cursor.eat(",");
        let case = cursor.int_until(b")")?;
        if !cursor.eat(")[") {
            return None;
        }
        let payload = parse_seq(cursor, b']')?;
        return Some(Value::Enum {
            event: u32::try_from(event).ok()?,
            case: u32::try_from(case).ok()?,
            payload,
        });
    }
    if cursor.eat("Record{") {
        let mut fields = Vec::new();
        if cursor.eat("}") {
            return Some(Value::Record(fields));
        }
        loop {
            let sym = cursor.int_until(b":")?;
            if !cursor.eat(":") {
                return None;
            }
            fields.push((u32::try_from(sym).ok()?, parse(cursor)?));
            if cursor.eat("}") {
                return Some(Value::Record(fields));
            }
            if !cursor.eat(",") {
                return None;
            }
        }
    }
    None
}

fn parse_seq(cursor: &mut Cursor<'_>, close: u8) -> Option<Vec<Value>> {
    let mut items = Vec::new();
    if cursor.text.get(cursor.pos) == Some(&close) {
        cursor.pos += 1;
        return Some(items);
    }
    loop {
        items.push(parse(cursor)?);
        match cursor.text.get(cursor.pos)? {
            b',' => cursor.pos += 1,
            b if *b == close => {
                cursor.pos += 1;
                return Some(items);
            }
            _ => return None,
        }
    }
}
