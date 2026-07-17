// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: queryd host-loopback proof — opcode round-trips over the real
//! capnp wire bytes, namespace isolation (two apps, same table id, disjoint
//! data), fail-closed permission denial, keyset paging through the wire.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 5 tests

use nexus_idl_runtime::queryspec_capnp as ws;
use queryd::{DenyAll, QuerydServer, StaticCaps, OP_CREATE_TABLE, OP_PUT, OP_QUERY};

fn frame<A: capnp::message::Allocator>(
    opcode: u8,
    message: &capnp::message::Builder<A>,
) -> Vec<u8> {
    let mut out = vec![opcode];
    capnp::serialize::write_message(&mut out, message).expect("write to vec");
    out
}

fn create_table_frame(table: u16) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut req = message.init_root::<ws::create_table_request::Builder<'_>>();
        req.set_table(table);
        req.set_pk_col(0);
        {
            let mut names = req.reborrow().init_names(3);
            names.set(0, capnp::text::Reader::from("id"));
            names.set(1, capnp::text::Reader::from("title"));
            names.set(2, capnp::text::Reader::from("rank"));
        }
        {
            let mut types = req.reborrow().init_types(3);
            types.set(0, ws::ColType::Int);
            types.set(1, ws::ColType::Str);
            types.set(2, ws::ColType::Int);
        }
        {
            let mut indexed = req.reborrow().init_indexed(1);
            indexed.set(0, 2);
        }
    }
    frame(OP_CREATE_TABLE, &message)
}

fn put_frame(table: u16, id: i64, title: &str, rank: i64) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    {
        let req = message.init_root::<ws::put_request::Builder<'_>>();
        let mut req = req;
        req.set_table(table);
        let mut row = req.init_row(3);
        row.reborrow().get(0).set_int_val(id);
        row.reborrow().get(1).set_str_val(capnp::text::Reader::from(title));
        row.reborrow().get(2).set_int_val(rank);
    }
    frame(OP_PUT, &message)
}

fn query_frame(table: u16, limit: u32, token: &[u8]) -> Vec<u8> {
    let mut message = capnp::message::Builder::new_default();
    {
        let mut req = message.init_root::<ws::query_request::Builder<'_>>();
        req.set_table(table);
        req.set_order_col(capnp::text::Reader::from("rank"));
        req.set_descending(false);
        req.set_limit(limit);
        req.set_token(token);
        req.init_preds(0);
    }
    frame(OP_QUERY, &message)
}

fn expect_ack_ok(bytes: &[u8]) {
    let message = capnp::serialize::read_message(bytes, Default::default()).expect("read");
    let ack = message.get_root::<ws::ack_response::Reader<'_>>().expect("root");
    assert!(
        matches!(ack.which(), Ok(ws::ack_response::Which::Ok(()))),
        "expected Ok ack, got {:?}",
        ack.which().ok().map(|w| matches!(w, ws::ack_response::Which::Err(_)))
    );
}

/// Returns (row pk ids, next token) from a QueryResponse.
fn read_page(bytes: &[u8]) -> (Vec<i64>, Vec<u8>) {
    let message = capnp::serialize::read_message(bytes, Default::default()).expect("read");
    let response = message.get_root::<ws::query_response::Reader<'_>>().expect("root");
    match response.which().expect("union") {
        ws::query_response::Which::Ok(page) => {
            let page = page.expect("page");
            let mut ids = Vec::new();
            for row in page.get_rows().expect("rows").iter() {
                let values = row.get_values().expect("values");
                match values.get(0).which().expect("val") {
                    ws::q_val::Which::IntVal(i) => ids.push(i),
                    _ => panic!("pk not an int"),
                }
            }
            (ids, page.get_next().expect("next").to_vec())
        }
        ws::query_response::Which::Err(e) => panic!("query failed: {e:?}"),
    }
}

fn read_query_err(bytes: &[u8]) -> ws::QueryErr {
    let message = capnp::serialize::read_message(bytes, Default::default()).expect("read");
    let response = message.get_root::<ws::query_response::Reader<'_>>().expect("root");
    match response.which().expect("union") {
        ws::query_response::Which::Err(e) => e.expect("err enum"),
        ws::query_response::Which::Ok(_) => panic!("expected error"),
    }
}

#[test]
fn opcode_round_trip_create_put_query() {
    let mut server = QuerydServer::new(StaticCaps::new(&["app.demo"]));
    expect_ack_ok(&server.handle_frame("app.demo", &create_table_frame(1)));
    for (id, rank) in [(1, 30), (2, 10), (3, 20)] {
        expect_ack_ok(&server.handle_frame("app.demo", &put_frame(1, id, "row", rank)));
    }
    let (ids, next) = read_page(&server.handle_frame("app.demo", &query_frame(1, 10, &[])));
    assert_eq!(ids, vec![2, 3, 1], "rank order");
    assert!(next.is_empty(), "exhausted");
}

#[test]
fn keyset_paging_walks_through_the_wire() {
    let mut server = QuerydServer::new(StaticCaps::new(&["app.demo"]));
    expect_ack_ok(&server.handle_frame("app.demo", &create_table_frame(1)));
    for id in 1..=7 {
        expect_ack_ok(&server.handle_frame("app.demo", &put_frame(1, id, "row", id * 10)));
    }
    let mut all = Vec::new();
    let mut token: Vec<u8> = Vec::new();
    loop {
        let (ids, next) = read_page(&server.handle_frame("app.demo", &query_frame(1, 3, &token)));
        all.extend(ids);
        if next.is_empty() {
            break;
        }
        token = next;
    }
    assert_eq!(all, vec![1, 2, 3, 4, 5, 6, 7]);
}

#[test]
fn namespaces_are_isolated_by_identity() {
    let mut server = QuerydServer::new(StaticCaps::new(&["app.a", "app.b"]));
    expect_ack_ok(&server.handle_frame("app.a", &create_table_frame(1)));
    expect_ack_ok(&server.handle_frame("app.b", &create_table_frame(1)));
    expect_ack_ok(&server.handle_frame("app.a", &put_frame(1, 1, "a-row", 1)));
    expect_ack_ok(&server.handle_frame("app.b", &put_frame(1, 2, "b-row", 1)));

    let (a_ids, _) = read_page(&server.handle_frame("app.a", &query_frame(1, 10, &[])));
    let (b_ids, _) = read_page(&server.handle_frame("app.b", &query_frame(1, 10, &[])));
    assert_eq!(a_ids, vec![1], "app.a sees only its own rows");
    assert_eq!(b_ids, vec![2], "app.b sees only its own rows");
}

#[test]
fn permission_gate_is_fail_closed() {
    // DenyAll: even a well-formed frame is refused before parsing.
    let mut server = QuerydServer::new(DenyAll);
    let err = read_query_err(&server.handle_frame("app.demo", &query_frame(1, 10, &[])));
    assert_eq!(err, ws::QueryErr::Denied);

    // An identity outside the allow-list is refused too.
    let mut server = QuerydServer::new(StaticCaps::new(&["app.other"]));
    let err = read_query_err(&server.handle_frame("app.demo", &query_frame(1, 10, &[])));
    assert_eq!(err, ws::QueryErr::Denied);
}

#[test]
fn malformed_frames_are_typed_errors() {
    let mut server = QuerydServer::new(StaticCaps::new(&["app.demo"]));
    // Unknown table.
    let err = read_query_err(&server.handle_frame("app.demo", &query_frame(9, 10, &[])));
    assert_eq!(err, ws::QueryErr::UnknownTable);
    // Garbage token.
    expect_ack_ok(&server.handle_frame("app.demo", &create_table_frame(1)));
    let err = read_query_err(&server.handle_frame("app.demo", &query_frame(1, 10, &[1, 2])));
    assert_eq!(err, ws::QueryErr::BadToken);
}
