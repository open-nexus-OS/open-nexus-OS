// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: TranscriptHost — deterministic record/replay of service
//! exchanges (docs/dev/dsl/services.md). Transcripts are checked-in text
//! fixtures; replay matches invocations IN ORDER and byte-exactly; a miss is
//! a recorded failure surfaced as [`ERR_TRANSCRIPT_MISS`] — never a silent
//! default. [`Recorder`] wraps any live host and emits the same format.
//! OWNERS: @ui @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 4 unit tests (round-trip, replay, miss, query entries)
//!
//! Format (one exchange per line, `#` comments):
//! ```text
//! # nx-transcript v1
//! call library.list() -> Ok(List[Str("Alpha")])
//! call db.put(Str("k"),Str("v")) -> Err(3)
//! query library(order=rank,desc=false,limit=20,token="",eq=,low=,high=Int(9)) -> Ok(next="",rows=List[])
//! ```

use super::value_text::{parse_value, value_to_text};
use crate::store::Value;
use crate::{EffectHost, QueryCall, QueryPage};
use alloc::{format, string::String, vec::Vec};

/// Stable error code returned on a replay miss (also recorded in `misses`).
pub const ERR_TRANSCRIPT_MISS: u32 = u32::MAX - 2;

enum Entry {
    Call { invocation: String, response: Result<Value, u32> },
    Query { invocation: String, response: Result<QueryPage, u32> },
}

/// Replays a parsed transcript. Construction fails on any malformed line —
/// a broken fixture is a build/test failure at load time.
pub struct TranscriptHost {
    entries: Vec<Entry>,
    next: usize,
    /// Replay misses (expected vs actual invocation text).
    pub misses: Vec<(String, String)>,
}

impl TranscriptHost {
    /// Parses the transcript text.
    ///
    /// # Errors
    /// The 1-based line number of the first malformed line.
    pub fn parse(text: &str) -> Result<Self, usize> {
        let mut entries = Vec::new();
        for (number, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            entries.push(parse_line(line).ok_or(number + 1)?);
        }
        Ok(Self { entries, next: 0, misses: Vec::new() })
    }

    /// True when every entry replayed in order with zero misses.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.misses.is_empty() && self.next == self.entries.len()
    }

    fn replay_call(&mut self, invocation: &str) -> Result<Value, u32> {
        match self.entries.get(self.next) {
            Some(Entry::Call { invocation: expected, response }) if expected == invocation => {
                self.next += 1;
                response.clone()
            }
            Some(Entry::Call { invocation: expected, .. })
            | Some(Entry::Query { invocation: expected, .. }) => {
                self.misses.push((expected.clone(), String::from(invocation)));
                Err(ERR_TRANSCRIPT_MISS)
            }
            None => {
                self.misses.push((String::from("<end of transcript>"), String::from(invocation)));
                Err(ERR_TRANSCRIPT_MISS)
            }
        }
    }

    fn replay_query(&mut self, invocation: &str) -> Result<QueryPage, u32> {
        match self.entries.get(self.next) {
            Some(Entry::Query { invocation: expected, response }) if expected == invocation => {
                self.next += 1;
                response.clone()
            }
            Some(Entry::Call { invocation: expected, .. })
            | Some(Entry::Query { invocation: expected, .. }) => {
                self.misses.push((expected.clone(), String::from(invocation)));
                Err(ERR_TRANSCRIPT_MISS)
            }
            None => {
                self.misses.push((String::from("<end of transcript>"), String::from(invocation)));
                Err(ERR_TRANSCRIPT_MISS)
            }
        }
    }
}

impl EffectHost for TranscriptHost {
    fn call(
        &mut self,
        service: &str,
        method: &str,
        args: &[Value],
        _timeout_ms: u32,
    ) -> Result<Value, u32> {
        let invocation = call_text(service, method, args);
        self.replay_call(&invocation)
    }

    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        let invocation = query_text(call);
        self.replay_query(&invocation)
    }
}

/// Wraps a live host and records every exchange in transcript format
/// (explicit dev flow — recordings become checked-in fixtures).
pub struct Recorder<'a> {
    pub inner: &'a mut dyn EffectHost,
    pub lines: Vec<String>,
}

impl<'a> Recorder<'a> {
    pub fn new(inner: &'a mut dyn EffectHost) -> Self {
        Self { inner, lines: alloc::vec![String::from("# nx-transcript v1")] }
    }

    /// The transcript text (stable line order = exchange order).
    #[must_use]
    pub fn transcript(&self) -> String {
        let mut out = self.lines.join("\n");
        out.push('\n');
        out
    }
}

impl EffectHost for Recorder<'_> {
    fn call(
        &mut self,
        service: &str,
        method: &str,
        args: &[Value],
        timeout_ms: u32,
    ) -> Result<Value, u32> {
        let response = self.inner.call(service, method, args, timeout_ms);
        let rhs = match &response {
            Ok(value) => format!("Ok({})", value_to_text(value)),
            Err(code) => format!("Err({code})"),
        };
        self.lines.push(format!("{} -> {rhs}", call_text(service, method, args)));
        response
    }

    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        let response = self.inner.query(call);
        let rhs = match &response {
            Ok(page) => format!("Ok(next=\"{}\",rows={})", page.next, value_to_text(&page.rows)),
            Err(code) => format!("Err({code})"),
        };
        self.lines.push(format!("{} -> {rhs}", query_text(call)));
        response
    }
}

// ------------------------------------------------------------- line format

fn call_text(service: &str, method: &str, args: &[Value]) -> String {
    let mut out = format!("call {service}.{method}(");
    for (i, arg) in args.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&value_to_text(arg));
    }
    out.push(')');
    out
}

fn query_text(call: &QueryCall) -> String {
    let mut out = format!(
        "query {}(order={},desc={},limit={},token=\"{}\",eq=",
        call.source, call.order_col, call.descending, call.limit, call.token
    );
    for (i, (col, value)) in call.eq.iter().enumerate() {
        if i > 0 {
            out.push(';');
        }
        out.push_str(&format!("{col}={}", value_to_text(value)));
    }
    out.push_str(",low=");
    if let Some(low) = &call.low {
        out.push_str(&value_to_text(low));
    }
    out.push_str(",high=");
    if let Some(high) = &call.high {
        out.push_str(&value_to_text(high));
    }
    out.push(')');
    out
}

fn parse_line(line: &str) -> Option<Entry> {
    let (invocation, response) = line.split_once(" -> ")?;
    if invocation.starts_with("call ") {
        let response = if let Some(rest) = response.strip_prefix("Ok(") {
            Ok(parse_value(rest.strip_suffix(')')?)?)
        } else if let Some(rest) = response.strip_prefix("Err(") {
            Err(rest.strip_suffix(')')?.parse().ok()?)
        } else {
            return None;
        };
        return Some(Entry::Call { invocation: String::from(invocation), response });
    }
    if invocation.starts_with("query ") {
        let response = if let Some(rest) = response.strip_prefix("Ok(next=\"") {
            let (next, rows_part) = rest.split_once("\",rows=")?;
            let rows = parse_value(rows_part.strip_suffix(')')?)?;
            Ok(QueryPage { rows, next: String::from(next) })
        } else if let Some(rest) = response.strip_prefix("Err(") {
            Err(rest.strip_suffix(')')?.parse().ok()?)
        } else {
            return None;
        };
        return Some(Entry::Query { invocation: String::from(invocation), response });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_round_trip_through_text() {
        let values = [
            Value::Unit,
            Value::Bool(true),
            Value::Int(-42),
            Value::Fx(1 << 33),
            Value::Str(String::from("a \"quoted\"\nline\\end")),
            Value::List(alloc::vec![Value::Int(1), Value::Str(String::from("x"))]),
            Value::Enum { event: 2, case: 1, payload: alloc::vec![Value::Bool(false)] },
            Value::Record(alloc::vec![(3, Value::Int(7)), (9, Value::Unit)]),
        ];
        for value in values {
            let text = value_to_text(&value);
            assert_eq!(parse_value(&text), Some(value.clone()), "text: {text}");
        }
        assert_eq!(parse_value("Str(\"unterminated"), None);
        assert_eq!(parse_value("Int(5) trailing"), None);
    }

    #[test]
    fn record_then_replay_is_faithful() {
        struct Fixed;
        impl EffectHost for Fixed {
            fn call(&mut self, _: &str, _: &str, _: &[Value], _: u32) -> Result<Value, u32> {
                Ok(Value::List(alloc::vec![Value::Str(String::from("Alpha"))]))
            }
        }
        let mut live = Fixed;
        let mut recorder = Recorder::new(&mut live);
        let args = [Value::Str(String::from("q"))];
        let recorded = recorder.call("library", "list", &args, 250);
        let text = recorder.transcript();

        let mut replay = TranscriptHost::parse(&text).expect("parses");
        let replayed = replay.call("library", "list", &args, 250);
        assert_eq!(recorded, replayed);
        assert!(replay.is_clean());
    }

    #[test]
    fn a_replay_miss_is_a_hard_failure_never_a_default() {
        let text = "# nx-transcript v1\ncall library.list() -> Ok(List[])\n";
        let mut host = TranscriptHost::parse(text).expect("parses");
        // Wrong method: miss recorded, distinguished error returned.
        let result = host.call("library", "get", &[], 250);
        assert_eq!(result, Err(ERR_TRANSCRIPT_MISS));
        assert!(!host.is_clean());
        assert_eq!(host.misses.len(), 1);
    }

    #[test]
    fn query_entries_replay_and_reject_mismatched_specs() {
        let text = "query items(order=rank,desc=false,limit=5,token=\"\",eq=,low=,high=) \
                    -> Ok(next=\"t1\",rows=List[Int(1)])\n";
        let mut host = TranscriptHost::parse(text).expect("parses");
        let call = QueryCall {
            source: String::from("items"),
            eq: Vec::new(),
            low: None,
            high: None,
            order_col: String::from("rank"),
            descending: false,
            limit: 5,
            token: String::new(),
        };
        let page = host.query(&call).expect("replays");
        assert_eq!(page.next, "t1");
        assert!(host.is_clean());

        // A different limit is a DIFFERENT query — miss.
        let mut host = TranscriptHost::parse(text).expect("parses");
        let other = QueryCall { limit: 6, ..call };
        assert_eq!(host.query(&other), Err(ERR_TRANSCRIPT_MISS));
        assert!(!host.is_clean());
    }
}
