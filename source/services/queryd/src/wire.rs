// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: queryd wire codec — opcode constants + capnp ↔ engine value
//! mapping for the queryspec.capnp contract. Pure functions, no state.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Exercised end-to-end by tests/loopback.rs

use nexus_idl_runtime::queryspec_capnp as ws;
use nexus_query::{QType, QVal, QueryError};

pub const OP_CREATE_TABLE: u8 = 1;
pub const OP_PUT: u8 = 2;
pub const OP_DELETE: u8 = 3;
pub const OP_QUERY: u8 = 4;

/// Wire → engine value.
pub(crate) fn decode_qval(reader: ws::q_val::Reader<'_>) -> Result<QVal, QueryError> {
    match reader.which().map_err(|_| QueryError::Unsupported)? {
        ws::q_val::Which::BoolVal(b) => Ok(QVal::Bool(b)),
        ws::q_val::Which::IntVal(i) => Ok(QVal::Int(i)),
        ws::q_val::Which::FxVal(f) => Ok(QVal::Fx(f)),
        ws::q_val::Which::StrVal(s) => Ok(QVal::Str(String::from(
            s.map_err(|_| QueryError::Corrupt)?
                .to_str()
                .map_err(|_| QueryError::Corrupt)?,
        ))),
    }
}

/// Engine → wire value.
pub(crate) fn encode_qval(value: &QVal, mut builder: ws::q_val::Builder<'_>) {
    match value {
        QVal::Bool(b) => builder.set_bool_val(*b),
        QVal::Int(i) => builder.set_int_val(*i),
        QVal::Fx(f) => builder.set_fx_val(*f),
        QVal::Str(s) => builder.set_str_val(capnp::text::Reader::from(s.as_str())),
    }
}

pub(crate) fn decode_col_type(ty: ws::ColType) -> QType {
    match ty {
        ws::ColType::Bool => QType::Bool,
        ws::ColType::Int => QType::Int,
        ws::ColType::Fx => QType::Fx,
        ws::ColType::Str => QType::Str,
    }
}

/// Engine/service error → the wire vocabulary.
pub(crate) fn encode_err(err: QueryError) -> ws::QueryErr {
    match err {
        QueryError::UnknownTable => ws::QueryErr::UnknownTable,
        QueryError::UnknownColumn => ws::QueryErr::UnknownColumn,
        QueryError::TypeMismatch => ws::QueryErr::TypeMismatch,
        QueryError::Unsupported => ws::QueryErr::Unsupported,
        QueryError::BadToken => ws::QueryErr::BadToken,
        QueryError::Corrupt => ws::QueryErr::Corrupt,
    }
}

/// Serializes one capnp message to its canonical flat bytes.
pub(crate) fn to_bytes<A: capnp::message::Allocator>(
    message: &capnp::message::Builder<A>,
) -> Vec<u8> {
    let mut out = Vec::new();
    // Writing to a Vec cannot fail.
    let _ = capnp::serialize::write_message(&mut out, message);
    out
}
