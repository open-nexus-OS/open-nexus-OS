// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! RFC-0077 locale-pack goldens: NXL1 encode/parse round-trip, NXLC
//! container split, fail-closed rejects, and the runtime swap-reemit proof.

use std::collections::BTreeMap;

use dsl_goldens::compile;
use nexus_dsl_core::{compile_project_bundle, encode_locale_pack};
use nexus_dsl_runtime::i18n::{parse_payload_container, Catalog};
use nexus_dsl_runtime::{FixtureEnv, LocaleChain, View};
use nexus_theme_tokens::BaseTokens;

fn keys(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn pack_encode_parse_round_trip_and_golden_bytes() {
    let mut entries = BTreeMap::new();
    entries.insert("greet.hello".to_string(), "Hallo {0}".to_string());
    let pack = encode_locale_pack(&keys(&["greet.bye", "greet.hello"]), &entries).expect("encodes");
    // Golden bytes: magic, count=2, absent, present("Hallo {0}").
    assert_eq!(&pack[..4], b"NXL1");
    assert_eq!(&pack[4..8], &2u32.to_le_bytes());
    assert_eq!(pack[8], 0, "greet.bye absent");
    assert_eq!(pack[9], 1, "greet.hello present");
    let catalog = Catalog::from_indexed_pack(&pack).expect("parses");
    // Index-aligned: key 0 falls through, key 1 resolves.
    let chain_keys = vec!["greet.bye".to_string(), "greet.hello".to_string()];
    let cats = [&catalog];
    let chain = LocaleChain::new(&cats, &chain_keys);
    use nexus_dsl_runtime::{LocaleSource, Value};
    assert_eq!(chain.format(1, &[Value::Str("Welt".into())]), "Hallo Welt");
    assert_eq!(chain.format(0, &[]), "greet.bye", "absent falls to pseudo terminal");
}

#[test]
fn test_reject_malformed_packs_fail_closed() {
    let mut entries = BTreeMap::new();
    entries.insert("k".to_string(), "v".to_string());
    let pack = encode_locale_pack(&keys(&["k"]), &entries).expect("encodes");
    assert!(Catalog::from_indexed_pack(&pack).is_some());
    // Truncations at every length.
    for n in 0..pack.len() {
        assert!(Catalog::from_indexed_pack(&pack[..n]).is_none(), "truncation {n}");
    }
    // Header mutations.
    let mut bad = pack.clone();
    bad[0] = b'X';
    assert!(Catalog::from_indexed_pack(&bad).is_none());
    let mut lying = pack.clone();
    lying[4] = 99; // count lies
    assert!(Catalog::from_indexed_pack(&lying).is_none());
    // Trailing garbage rejects (exact-length contract).
    let mut extra = pack.clone();
    extra.push(0);
    assert!(Catalog::from_indexed_pack(&extra).is_none());
}

#[test]
fn container_splits_nxir_and_packs_from_a_real_project() {
    // A real on-disk project: two catalogs, one key.
    let dir = std::env::temp_dir().join(format!("nxlc-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ui/pages")).unwrap();
    std::fs::create_dir_all(dir.join("i18n")).unwrap();
    std::fs::write(
        dir.join("ui/pages/Main.nx"),
        "Page Main {\n    Stack {\n        Text(@t(\"app.title\"))\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("i18n/en.json"), "{\n  \"app.title\": \"Hello\"\n}\n").unwrap();
    std::fs::write(dir.join("i18n/de.json"), "{\n  \"app.title\": \"Hallo\"\n}\n").unwrap();
    let payload = compile_project_bundle(&dir).expect("bundle compiles");
    let (nxir, packs) = parse_payload_container(&payload).expect("container parses");
    assert_eq!(packs.len(), 2, "de + en packs");
    assert_eq!(packs[0].tag, "de");
    assert_eq!(packs[1].tag, "en");
    // The de pack resolves the key; the NXIR mounts and renders the BAKED
    // default ("Hello"), then a swap to de re-renders "Hallo".
    let de = Catalog::from_indexed_pack(packs[0].pack).expect("de pack parses");
    let nxir_owned = nxir.to_vec();
    let symbols = nexus_dsl_runtime::Runtime::mount(&nxir_owned).unwrap().symbols().to_vec();
    let keys_tbl = dsl_goldens::i18n_keys(&nxir_owned);
    let baked = nexus_dsl_runtime::IdentityLocale { symbols: &symbols, keys: &keys_tbl };
    let mut view =
        View::mount(&nxir_owned, &BaseTokens, &FixtureEnv::default(), &baked).expect("mounts");
    assert!(dsl_goldens::texts(view.scene()).contains(&"Hello".to_string()), "baked default");
    // Swap: active catalog wins, reemit re-renders.
    let names = vec![String::from("app.title")];
    let cats = [&de];
    let chain = LocaleChain::new(&cats, &names);
    view.reemit(&BaseTokens, &FixtureEnv::default(), &chain).expect("reemit");
    assert!(
        dsl_goldens::texts(view.scene()).contains(&"Hallo".to_string()),
        "switched to de: {:?}",
        dsl_goldens::texts(view.scene())
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn packless_project_stays_raw_nxir() {
    let dir = std::env::temp_dir().join(format!("nxlc-raw-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ui/pages")).unwrap();
    std::fs::write(
        dir.join("ui/pages/Main.nx"),
        "Page Main {\n    Stack {\n        Text(\"plain\")\n    }\n}\n",
    )
    .unwrap();
    let payload = compile_project_bundle(&dir).expect("compiles");
    assert!(parse_payload_container(&payload).is_none(), "raw NXIR, no container");
    // Sanity: the identical program via the raw path is byte-identical.
    let raw = nexus_dsl_core::compile_project_dir(&dir).expect("raw compiles");
    assert_eq!(payload, raw);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_reject_malformed_containers() {
    let dir = std::env::temp_dir().join(format!("nxlc-rej-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("ui/pages")).unwrap();
    std::fs::create_dir_all(dir.join("i18n")).unwrap();
    std::fs::write(
        dir.join("ui/pages/Main.nx"),
        "Page Main {\n    Stack {\n        Text(@t(\"k\"))\n    }\n}\n",
    )
    .unwrap();
    std::fs::write(dir.join("i18n/de.json"), "{\n  \"k\": \"v\"\n}\n").unwrap();
    let payload = compile_project_bundle(&dir).expect("compiles");
    assert!(parse_payload_container(&payload).is_some());
    for n in [0, 3, 8, 11, payload.len() - 1] {
        assert!(parse_payload_container(&payload[..n]).is_none(), "truncation {n}");
    }
    let mut bad = payload.clone();
    bad[4] = 9; // unknown version
    assert!(parse_payload_container(&bad).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn compile_helper_smoke() {
    // Keep the shared compile() helper covered from this file too.
    let nxir = compile("Page P {\n    Stack {\n        Text(\"x\")\n    }\n}\n");
    assert!(!nxir.is_empty());
}
