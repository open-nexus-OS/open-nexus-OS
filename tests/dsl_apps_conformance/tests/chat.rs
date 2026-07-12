// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
// QuerySpec-fed transcript (Scroll-Track S4): the REAL `nexus-query` engine
// (keyset paging) backs `EffectHost::query()` — the same shape the app-host
// embeds on-device. The mount root effect loads page 1; `LoadMore` (what the
// scroll container's `on EndReached` dispatches) appends page 2 resuming
// from the continuation token.

mod common;

use nexus_dsl_runtime::{
    EffectHost, FixtureEnv, IdentityLocale, QueryCall, QueryPage, Value, View,
};
use nexus_query::{Engine, MemKv, PageToken, QType, QVal, QuerySpec, TableDef};

/// In-process query host over the platform engine — the on-device
/// `AppEffectHost::query()` twin (messages table: seq Int pk, text Str).
struct TranscriptHost {
    engine: Engine,
    kv: MemKv,
    seq_sym: u32,
    text_sym: u32,
}

impl TranscriptHost {
    fn seeded(symbols: &[String], n: i64) -> Self {
        let engine = Engine::new(vec![TableDef {
            id: 0,
            columns: vec![QType::Int, QType::Str],
            pk_col: 0,
            indexed: vec![0],
        }]);
        let mut kv = MemKv::new();
        for seq in 1..=n {
            engine
                .put(&mut kv, 0, &[QVal::Int(seq), QVal::Str(format!("msg {seq}"))])
                .expect("seed row");
        }
        let sym = |name: &str| symbols.iter().position(|s| s == name).expect(name) as u32;
        Self { engine, kv, seq_sym: sym("seq"), text_sym: sym("text") }
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
    if text.len() % 2 != 0 {
        return None;
    }
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

impl EffectHost for TranscriptHost {
    fn call(&mut self, _: &str, _: &str, _: &[Value], _: u32) -> Result<Value, u32> {
        Err(u32::MAX)
    }

    fn query(&mut self, call: &QueryCall) -> Result<QueryPage, u32> {
        assert_eq!(call.source, "messages");
        let spec = QuerySpec {
            table: 0,
            eq: vec![],
            range: None,
            order_col: 0,
            descending: call.descending,
            limit: call.limit,
        };
        let token = if call.token.is_empty() {
            None
        } else {
            Some(PageToken::from_bytes(&hex_decode(&call.token).unwrap()).unwrap())
        };
        let page = self.engine.query(&self.kv, &spec, token.as_ref()).map_err(|_| 9u32)?;
        let rows: Vec<Value> = page
            .rows
            .into_iter()
            .map(|row| {
                let mut fields: Vec<(u32, Value)> = row
                    .into_iter()
                    .enumerate()
                    .map(|(i, qv)| {
                        let sym = if i == 0 { self.seq_sym } else { self.text_sym };
                        let v = match qv {
                            QVal::Int(n) => Value::Int(n),
                            QVal::Str(s) => Value::Str(s),
                            QVal::Bool(b) => Value::Bool(b),
                            QVal::Fx(f) => Value::Fx(f),
                        };
                        (sym, v)
                    })
                    .collect();
                fields.sort_by_key(|(s, _)| *s);
                Value::Record(fields)
            })
            .collect();
        Ok(QueryPage {
            rows: Value::List(rows),
            next: page.next.map(|t| hex_encode(t.as_bytes())).unwrap_or_default(),
        })
    }
}

#[test]
fn chat_transcript_pages_lazily() {
    let nxir = common::compile("chat");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = TranscriptHost::seeded(&symbols, 240);

    // Root effect (LoadFirst) fires at mount: the FIRST page (limit 60) —
    // and ONLY that window — is resident.
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");
    let texts = common::scene_texts(&view);
    assert!(texts.iter().any(|t| t == "msg 1"), "first page missing: got {} texts", texts.len());
    assert!(texts.iter().any(|t| t == "msg 60"), "page tail missing");
    assert!(!texts.iter().any(|t| t == "msg 61"), "loaded past the window");

    // Lazy loading: EndReached → LoadMore resumes from the keyset token.
    common::dispatch(&mut view, &device, &mut host, &symbols, "ChatEvent", "LoadMore", vec![]);
    let texts = common::scene_texts(&view);
    assert!(texts.iter().any(|t| t == "msg 61"), "second page missing");
    assert!(texts.iter().any(|t| t == "msg 120"), "second page tail missing");
    assert!(!texts.iter().any(|t| t == "msg 121"), "loaded past the second window");
}

/// Store-window: `tail(messages, 64)` keeps only the last 64 messages resident
/// no matter how far the transcript is paged. Paging a 400-message source to
/// the end trims the head — emit/layout/paint and the store concat stay
/// O(window), which is what lifts the former ~120-message re-emit cap (the
/// whole-scene re-emit on the app's non-freeing bump heap).
#[test]
fn chat_transcript_windows_to_last_64() {
    let nxir = common::compile("chat");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = TranscriptHost::seeded(&symbols, 400);

    // Mount loads 1..60; six LoadMore pages (60 each) reach seq 400, at which
    // point the continuation token is empty. Dispatch EXACTLY that many — an
    // extra LoadMore on an empty token would restart paging from seq 1.
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");
    for _ in 0..6 {
        common::dispatch(&mut view, &device, &mut host, &symbols, "ChatEvent", "LoadMore", vec![]);
    }

    let texts = common::scene_texts(&view);
    let resident = texts.iter().filter(|t| t.starts_with("msg ")).count();
    // Newest resident, window floor (400-64+1 = 337) present, older trimmed.
    assert!(texts.iter().any(|t| t == "msg 400"), "newest message missing");
    assert!(texts.iter().any(|t| t == "msg 337"), "window floor (msg 337) missing");
    assert!(!texts.iter().any(|t| t == "msg 336"), "message past the 64-window still resident");
    assert!(!texts.iter().any(|t| t == "msg 1"), "head not trimmed");
    assert!(resident <= 64, "resident window {resident} exceeds cap 64");
}

/// The transcript viewport is a real `.scroll` container: the engine stamps
/// `clip_rect` on its descendants (what the paint-time offset keys on).
#[test]
fn chat_transcript_is_clipped_scroll_viewport() {
    let nxir = common::compile("chat");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = TranscriptHost::seeded(&symbols, 240);
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("initial effects");

    let boxes = common::layout_boxes(&view);
    let clipped = boxes.iter().filter(|b| b.clip_rect.is_some()).count();
    assert!(clipped > 0, "no clipped boxes — .scroll(vertical) did not clip");
}

/// The scroll viewport's layout contract: chrome outside stays unclipped at
/// full size, rows inside keep their content height (never flex-shrunk), and
/// the content extent exceeds the viewport (there IS something to scroll).
#[test]
fn diag_clip_stamping() {
    let nxir = common::compile("chat");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = TranscriptHost::seeded(&symbols, 240);
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = common::layout_boxes(&view);
    let total = boxes.len();
    let clipped: Vec<_> = boxes.iter().filter(|b| b.clip_rect.is_some()).collect();
    eprintln!("total={} clipped={}", total, clipped.len());
    for b in boxes.iter().take(12) {
        eprintln!(
            "id={} y={} h={} clip={:?}",
            b.node_id, b.rect.y.0, b.rect.height.0,
            b.clip_rect.map(|c| (c.x.0, c.y.0, c.width.0, c.height.0))
        );
    }
    // The container itself + rows outside it must NOT be clipped away later:
    let uniq: std::collections::BTreeSet<_> = clipped
        .iter()
        .filter_map(|b| b.clip_rect.map(|c| (c.x.0, c.y.0, c.width.0, c.height.0)))
        .collect();
    assert_eq!(uniq.len(), 1, "one scroll viewport expected: {uniq:?}");
    let (_, cy, _, ch) = *uniq.iter().next().unwrap();
    // Chrome outside the viewport is unclipped and full-size (the toolbar
    // collapsed to 12px/-4px rows before the Scroll measurement fix).
    let toolbar = boxes.iter().find(|b| b.node_id == 2).expect("toolbar box");
    assert!(toolbar.clip_rect.is_none() && toolbar.rect.height.0 > 30, "toolbar squeezed");
    // Rows keep content height; the content extent exceeds the viewport.
    let content_bottom =
        clipped.iter().map(|b| b.rect.y.0 + b.rect.height.0).max().unwrap_or(0);
    assert!(clipped.iter().all(|b| b.rect.height.0 >= 0), "negative row heights");
    assert!(content_bottom > cy + ch, "content does not overflow the viewport");
}

/// `EndReached` is a CONTAINER-scoped event: dispatched by NAME (the
/// app-host's `fire_end_reached`), never by hit-test — it must load the next
/// page regardless of how far the content is panned.
#[test]
fn end_reached_handler_is_hittable() {
    let nxir = common::compile("chat");
    let symbols = common::program_symbols(&nxir);
    let tokens = nexus_theme_tokens::BaseTokens;
    let device = FixtureEnv::tablet("landscape");
    let keys: Vec<u32> = Vec::new();
    let locale = IdentityLocale { symbols: &symbols, keys: &keys };
    let mut view = View::mount(&nxir, &tokens, &device, &locale).expect("mounts");
    let mut host = TranscriptHost::seeded(&symbols, 240);
    view.run_initial_effects(&tokens, &device, &locale, &mut host).expect("effects");
    let boxes = common::layout_boxes(&view);
    let clip = boxes
        .iter()
        .find_map(|b| b.clip_rect)
        .expect("scroll viewport");
    let (cx, cy) = (clip.x.0 + clip.width.0 / 2, clip.y.0 + clip.height.0 / 2);
    let _ = (cx, cy);
    let damage = view
        .fire_trigger(&tokens, &device, &locale, &mut host, "EndReached")
        .expect("fire");
    assert!(damage.is_some(), "EndReached handler not dispatched by name");
    let texts = common::scene_texts(&view);
    assert!(texts.iter().any(|t| t == "msg 61"), "EndReached did not load page 2");
}
