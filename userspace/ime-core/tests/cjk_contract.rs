// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! TASK-0149 CJK engine contract: conversion goldens (JP/KR/ZH), candidate
//! ordering + paging, user-dict determinism, engine swap behind the ONE
//! trait, and a no-panic random-stream soak. Deterministic across runs by
//! construction (const tables, fixed LCG seed).

use ime_core::{
    Engine, EngineId, ImeAction, ImeEngine, ImeKey, JpEngine, KrEngine, UserDict, ZhEngine,
    CANDIDATE_PAGE_MAX,
};

fn feed_str(engine: &mut impl ImeEngine, text: &str) -> String {
    let mut committed = String::new();
    for ch in text.chars() {
        let out = engine.feed(ImeKey::Text(ch));
        committed.push_str(out.commit.as_str());
    }
    committed
}

fn ranked<const N: usize>(dict: &UserDict<N>, key: &str) -> Vec<String> {
    let mut out = [""; 8];
    let n = dict.lookup(key, &mut out);
    out[..n].iter().map(ToString::to_string).collect()
}

// ---------------------------------------------------------------- japanese

#[test]
fn jp_nihongo_composes_and_ranks_nihongo_first() {
    let mut jp = JpEngine::new();
    let _ = feed_str(&mut jp, "nihongo");
    let out = jp.feed(ImeKey::Text(' ')); // open candidates
    assert!(out.handled);
    assert_eq!(out.preedit.as_str(), "にほんご");
    assert_eq!(out.candidates.get(0).map(|c| c.as_str()), Some("日本語"));
    // The kana reading is always the LAST candidate (honest fallback).
    let last = out.candidates.len() - 1;
    assert_eq!(out.candidates.get(last).map(|c| c.as_str()), Some("にほんご"));
    // Selecting the kanji commits it and clears the session.
    let sel = jp.select(0);
    assert_eq!(sel.commit.as_str(), "日本語");
    assert_eq!(sel.preedit.as_str(), "");
}

#[test]
fn jp_sokuon_and_n_edge_cases() {
    // Doubled consonant → っ.
    let mut jp = JpEngine::new();
    let _ = feed_str(&mut jp, "kitte");
    let out = jp.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "きって");

    // `n` before a consonant → ん (kanji), `nn` → ん.
    let mut jp = JpEngine::new();
    let _ = feed_str(&mut jp, "kanji");
    let out = jp.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "かんじ");

    let mut jp = JpEngine::new();
    let _ = feed_str(&mut jp, "nn");
    let out = jp.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "ん");

    // Trailing lone `n` resolves to ん on the FINAL commit (no more input
    // can follow, so the ambiguity is gone).
    let mut jp = JpEngine::new();
    let _ = feed_str(&mut jp, "mikan");
    let out = jp.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "みかん");
}

#[test]
fn jp_backspace_edits_composition_not_the_field() {
    let mut jp = JpEngine::new();
    let _ = feed_str(&mut jp, "nih"); // に + pending romaji "h"
    let out = jp.feed(ImeKey::Action(ImeAction::Backspace));
    assert!(out.handled);
    assert_eq!(out.preedit.as_str(), "に"); // romaji tail popped first
                                            // Draining the composition: further backspaces stay handled…
    let out = jp.feed(ImeKey::Action(ImeAction::Backspace));
    assert!(out.handled);
    assert_eq!(out.preedit.as_str(), "");
    // …and an EMPTY engine passes backspace to the field (unhandled).
    let out = jp.feed(ImeKey::Action(ImeAction::Backspace));
    assert!(!out.handled);
}

// ------------------------------------------------------------------ korean

#[test]
fn kr_han_from_jamo_and_backspace_splits() {
    let mut kr = KrEngine::new();
    // ㅎ + ㅏ + ㄴ → 한 (fed as jamo directly).
    let _ = kr.feed(ImeKey::Text('ㅎ'));
    let _ = kr.feed(ImeKey::Text('ㅏ'));
    let out = kr.feed(ImeKey::Text('ㄴ'));
    assert_eq!(out.preedit.as_str(), "한");
    // Backspace splits the final jamo: 한 → 하.
    let out = kr.feed(ImeKey::Action(ImeAction::Backspace));
    assert_eq!(out.preedit.as_str(), "하");
}

#[test]
fn kr_latin_two_set_and_jong_steal() {
    let mut kr = KrEngine::new();
    // "gks" = ㅎㅏㄴ = 한; then "k" (ㅏ) steals the ㄴ → 하나.
    let _ = feed_str(&mut kr, "gks");
    let out = kr.feed(ImeKey::Text('k'));
    assert_eq!(out.preedit.as_str(), "하나");
    let out = kr.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "하나");
}

#[test]
fn kr_compound_vowel_and_final() {
    let mut kr = KrEngine::new();
    // d h k = ㅇ ㅗ ㅏ → 와 (compound medial ㅘ).
    let _ = feed_str(&mut kr, "dhk");
    let out = kr.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "와");

    // 닭: e k f r = ㄷ ㅏ ㄹ ㄱ (compound final ㄺ).
    let mut kr = KrEngine::new();
    let _ = feed_str(&mut kr, "ekfr");
    let out = kr.feed(ImeKey::Action(ImeAction::Enter));
    assert_eq!(out.commit.as_str(), "닭");
}

// ----------------------------------------------------------------- chinese

#[test]
fn zh_nihao_ranks_first_and_selects() {
    let mut zh = ZhEngine::new();
    let _ = feed_str(&mut zh, "nihao");
    let out = zh.feed(ImeKey::Text(' '));
    assert_eq!(out.preedit.as_str(), "nihao");
    assert_eq!(out.candidates.get(0).map(|c| c.as_str()), Some("你好"));
    let sel = zh.select(0);
    assert_eq!(sel.commit.as_str(), "你好");
    assert_eq!(sel.preedit.as_str(), "");
}

#[test]
fn zh_paging_beyond_eight_candidates() {
    let mut zh = ZhEngine::new();
    let _ = feed_str(&mut zh, "shi");
    let out = zh.feed(ImeKey::Text(' '));
    assert_eq!(out.candidates.total, 10);
    assert_eq!(out.candidates.len(), CANDIDATE_PAGE_MAX);
    assert_eq!(out.candidates.get(0).map(|c| c.as_str()), Some("是"));
    // Page 2 holds the remaining two; page cursor wraps after the last.
    let out = zh.page_next();
    assert_eq!(out.candidates.page, 1);
    assert_eq!(out.candidates.len(), 2);
    assert_eq!(out.candidates.get(0).map(|c| c.as_str()), Some("施"));
    let out = zh.page_next();
    assert_eq!(out.candidates.page, 0);
    // Selecting from the CURRENT page commits that page's item.
    let out = zh.page_next();
    assert_eq!(out.candidates.page, 1);
    let sel = zh.select(1);
    assert_eq!(sel.commit.as_str(), "石");
}

// ---------------------------------------------------------------- userdict

#[test]
fn userdict_train_lookup_forget_deterministic() {
    let mut dict: UserDict<8> = UserDict::new();
    assert!(dict.train("nihao", "你好"));
    assert!(dict.train("nihao", "妮好"));
    assert!(dict.train("nihao", "你好")); // freq 2
    assert_eq!(ranked(&dict, "nihao"), ["你好", "妮好"]);
    // Equal frequency ties break by insertion order (earlier first).
    assert!(dict.train("nihao", "妮好")); // both at freq 2 now
    assert_eq!(ranked(&dict, "nihao"), ["你好", "妮好"]);
    assert!(dict.forget("nihao", "你好"));
    assert_eq!(ranked(&dict, "nihao"), ["妮好"]);
    assert!(!dict.forget("nihao", "你好"));
}

#[test]
fn userdict_eviction_lowest_freq_oldest_first() {
    let mut dict: UserDict<2> = UserDict::new();
    assert!(dict.train("a", "一")); // seq 0, freq 1
    assert!(dict.train("b", "二")); // seq 1, freq 1
    assert!(dict.train("b", "二")); // freq 2
                                    // Full: the next distinct pair evicts the lowest-freq OLDEST → ("a","一").
    assert!(dict.train("c", "三"));
    assert!(ranked(&dict, "a").is_empty());
    assert_eq!(ranked(&dict, "b"), ["二"]);
    assert_eq!(ranked(&dict, "c"), ["三"]);
    // Oversized inputs fail closed.
    assert!(!dict.train("", "x"));
    assert!(!dict.train(&"k".repeat(64), "x"));
}

// ----------------------------------------------------- engine swap + soak

#[test]
fn one_session_hosts_any_engine_behind_the_trait() {
    // The imed-side contract: ONE `Engine` value slot, re-seated per layout
    // change — no language-specific code outside ime-core.
    for (layout, input, commit) in
        [("jp", "nn", "ん"), ("kr", "gks", "한"), ("zh", "wo", "wo"), ("us", "x", "")]
    {
        let mut session = Engine::new(EngineId::for_layout(layout));
        let _ = feed_str(&mut session, input);
        let out = session.feed(ImeKey::Action(ImeAction::Enter));
        assert_eq!(out.commit.as_str(), commit, "layout {layout}");
    }
}

#[test]
fn random_streams_never_panic_and_stay_bounded() {
    // Fixed-seed LCG → deterministic "random" key soup.
    let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next = move || {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (state >> 33) as u32
    };
    let keys = |r: u32| -> ImeKey {
        match r % 7 {
            0 => ImeKey::Text((b'a' + (r / 7 % 26) as u8) as char),
            1 => ImeKey::Text(' '),
            2 => ImeKey::Text('ㅏ'),
            3 => ImeKey::Action(ImeAction::Backspace),
            4 => ImeKey::Action(ImeAction::Enter),
            5 => ImeKey::Dead('´'),
            _ => ImeKey::Action(ImeAction::Escape),
        }
    };
    for id in [EngineId::Latin, EngineId::Jp, EngineId::Kr, EngineId::Zh] {
        let mut engine = Engine::new(id);
        for _ in 0..10_000 {
            let r = next();
            let out = engine.feed(keys(r));
            assert!(out.preedit.as_str().len() <= 64);
            if r % 11 == 0 {
                let _ = engine.select((r % 9) as usize);
            }
            if r % 13 == 0 {
                let _ = engine.page_next();
            }
        }
    }
}
