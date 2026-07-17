// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Keyset-paging integration proof for nexus-query: walking a query
//! page-by-page yields EXACTLY the full-scan result (no duplicates, no gaps),
//! stays sane under interleaved writes, and the canonical query hash is
//! pinned so page tokens stay valid across engine releases.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 tests

use nexus_query::{Engine, MemKv, Page, PageToken, QType, QVal, QuerySpec, Range, TableDef};

const TABLE: u16 = 7;

fn table() -> TableDef {
    // (id Int pk, title Str, rank Int[indexed], flagged Bool)
    TableDef {
        id: TABLE,
        columns: vec![QType::Int, QType::Str, QType::Int, QType::Bool],
        pk_col: 0,
        indexed: vec![2],
    }
}

fn row(id: i64, rank: i64) -> Vec<QVal> {
    vec![QVal::Int(id), QVal::Str(format!("item-{id}")), QVal::Int(rank), QVal::Bool(id % 3 == 0)]
}

/// Deterministic but scrambled fixture: ids 1..=n inserted in a shuffled
/// order (sorted by a hash-ish key — a bijection, so ALL ids land), ranks
/// collide in groups (rank = id*7 % 23) so pk tiebreaks and duplicate
/// order-values are covered.
fn seeded(n: i64) -> (Engine, MemKv) {
    let engine = Engine::new(vec![table()]);
    let mut kv = MemKv::new();
    let mut order: Vec<i64> = (1..=n).collect();
    order.sort_by_key(|id| ((id * 7919) % 997, *id));
    for id in order {
        engine.put(&mut kv, TABLE, &row(id, (id * 7) % 23)).unwrap();
    }
    (engine, kv)
}

fn ids(page: &Page) -> Vec<i64> {
    page.rows
        .iter()
        .map(|r| match r[0] {
            QVal::Int(i) => i,
            _ => unreachable!(),
        })
        .collect()
}

fn walk(engine: &Engine, kv: &MemKv, spec: &QuerySpec) -> Vec<i64> {
    let mut all = Vec::new();
    let mut token: Option<PageToken> = None;
    let mut hops = 0;
    loop {
        let page = engine.query(kv, spec, token.as_ref()).unwrap();
        assert!(page.rows.len() <= spec.limit as usize, "page over limit");
        all.extend(ids(&page));
        match page.next {
            Some(t) => token = Some(t),
            None => return all,
        }
        hops += 1;
        assert!(hops < 1000, "walk did not terminate");
    }
}

fn full(engine: &Engine, kv: &MemKv, spec: &QuerySpec) -> Vec<i64> {
    let one_shot = QuerySpec { limit: 100_000, ..spec.clone() };
    let page = engine.query(kv, &one_shot, None).unwrap();
    assert!(page.next.is_none(), "one-shot must exhaust");
    ids(&page)
}

#[test]
fn paged_walk_equals_full_scan_ascending() {
    let (engine, kv) = seeded(97);
    for limit in [1u32, 2, 3, 7, 10, 96, 97, 200] {
        let spec = QuerySpec {
            table: TABLE,
            eq: vec![],
            range: None,
            order_col: 2,
            descending: false,
            limit,
        };
        assert_eq!(walk(&engine, &kv, &spec), full(&engine, &kv, &spec), "limit={limit}");
    }
}

#[test]
fn paged_walk_equals_full_scan_descending() {
    let (engine, kv) = seeded(97);
    for limit in [1u32, 3, 8, 97] {
        let spec = QuerySpec {
            table: TABLE,
            eq: vec![],
            range: None,
            order_col: 2,
            descending: true,
            limit,
        };
        let walked = walk(&engine, &kv, &spec);
        let mut expect = full(&engine, &kv, &spec);
        assert_eq!(walked, expect, "limit={limit}");
        // Descending really is the ascending order reversed.
        let asc = QuerySpec { descending: false, ..spec };
        expect = full(&engine, &kv, &asc);
        expect.reverse();
        assert_eq!(walked, expect);
    }
}

#[test]
fn paged_walk_with_eq_filter_and_range() {
    let (engine, kv) = seeded(97);
    let spec = QuerySpec {
        table: TABLE,
        eq: vec![(3, QVal::Bool(true))],
        range: Some(Range { low: Some(QVal::Int(5)), high: Some(QVal::Int(18)) }),
        order_col: 2,
        descending: false,
        limit: 4,
    };
    let walked = walk(&engine, &kv, &spec);
    assert_eq!(walked, full(&engine, &kv, &spec));
    assert!(!walked.is_empty(), "fixture must exercise the filter");
    // Every returned id satisfies both predicates.
    for id in &walked {
        assert_eq!(id % 3, 0);
        let rank = (id * 7) % 23;
        assert!((5..=18).contains(&rank));
    }
}

#[test]
fn pk_order_pages_identically() {
    let (engine, kv) = seeded(50);
    let spec = QuerySpec {
        table: TABLE,
        eq: vec![],
        range: Some(Range { low: Some(QVal::Int(10)), high: None }),
        order_col: 0,
        descending: false,
        limit: 7,
    };
    let walked = walk(&engine, &kv, &spec);
    assert_eq!(walked, full(&engine, &kv, &spec));
    let mut sorted = walked.clone();
    sorted.sort_unstable();
    assert_eq!(walked, sorted, "pk order is ascending id order");
    assert!(walked.iter().all(|&id| id >= 10));
}

/// Rows inserted BEHIND the cursor don't reappear; rows AHEAD are picked up —
/// the keyset contract (an offset cursor would duplicate or skip here).
#[test]
fn interleaved_writes_keep_keyset_contract() {
    let engine = Engine::new(vec![table()]);
    let mut kv = MemKv::new();
    for id in [10i64, 20, 30, 40] {
        engine.put(&mut kv, TABLE, &row(id, id)).unwrap();
    }
    let spec = QuerySpec {
        table: TABLE,
        eq: vec![],
        range: None,
        order_col: 2,
        descending: false,
        limit: 2,
    };
    let first = engine.query(&kv, &spec, None).unwrap();
    assert_eq!(ids(&first), vec![10, 20]);
    let token = first.next.expect("more pages");

    // Insert behind (rank 5) and ahead (rank 35) of the cursor.
    engine.put(&mut kv, TABLE, &row(5, 5)).unwrap();
    engine.put(&mut kv, TABLE, &row(35, 35)).unwrap();

    let second = engine.query(&kv, &spec, Some(&token)).unwrap();
    assert_eq!(ids(&second), vec![30, 35], "behind-cursor row must not resurface");
    let third = engine.query(&kv, &spec, Some(second.next.as_ref().expect("more"))).unwrap();
    assert_eq!(ids(&third), vec![40]);
    assert!(third.next.is_none());
}

/// The canonical hash is a WIRE COMMITMENT (tokens embed it) — pin its value.
#[test]
fn canonical_hash_is_pinned_and_order_independent() {
    let spec = QuerySpec {
        table: TABLE,
        eq: vec![(3, QVal::Bool(true)), (1, QVal::Str("x".into()))],
        range: Some(Range { low: Some(QVal::Int(5)), high: None }),
        order_col: 2,
        descending: false,
        limit: 4,
    };
    let swapped =
        QuerySpec { eq: vec![(1, QVal::Str("x".into())), (3, QVal::Bool(true))], ..spec.clone() };
    assert_eq!(spec.hash(), swapped.hash(), "eq order must not change identity");
    // Golden: recomputed only on a DOCUMENTED canonical-bytes change (ir.md-style).
    assert_eq!(spec.hash(), 0x724d_3c50_22ec_6e82, "canonical hash drifted");
    assert_ne!(spec.hash(), QuerySpec { limit: 5, ..spec.clone() }.hash());
}

#[test]
fn malformed_and_foreign_tokens_are_rejected() {
    let (engine, kv) = seeded(20);
    let spec = QuerySpec {
        table: TABLE,
        eq: vec![],
        range: None,
        order_col: 2,
        descending: false,
        limit: 3,
    };
    assert!(PageToken::from_bytes(&[1, 2, 3]).is_none(), "short token parses");
    // A token whose resume key lies outside this query's scan space = BadToken.
    let alien = PageToken::from_bytes(&{
        let mut b = spec.hash().to_le_bytes().to_vec();
        b.extend_from_slice(b"zzzz-not-a-key");
        b
    })
    .unwrap();
    assert_eq!(
        engine.query(&kv, &spec, Some(&alien)).unwrap_err(),
        nexus_query::QueryError::BadToken
    );
}

#[test]
fn tokens_round_trip_through_wire_bytes() {
    let (engine, kv) = seeded(30);
    let spec = QuerySpec {
        table: TABLE,
        eq: vec![],
        range: None,
        order_col: 2,
        descending: false,
        limit: 4,
    };
    let page = engine.query(&kv, &spec, None).unwrap();
    let token = page.next.expect("more pages");
    let wire = token.as_bytes().to_vec();
    let revived = PageToken::from_bytes(&wire).expect("wire token parses");
    let via_wire = engine.query(&kv, &spec, Some(&revived)).unwrap();
    let direct = engine.query(&kv, &spec, Some(&token)).unwrap();
    assert_eq!(ids(&via_wire), ids(&direct));
}
