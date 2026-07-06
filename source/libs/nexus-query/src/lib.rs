// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nexus-query — the pure-Rust structured query engine behind the
//! DSL's QuerySpec v1 (TASK-0078B): typed rows over any ordered KV, index-
//! driven ordered scans, keyset paging with hash-bound tokens. no_std+alloc;
//! the same engine runs on host tests (MemKv) and in queryd over statefsd.
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 21 tests (encoding properties, engine unit, paging walks)
//!
//! Layering (each file one concern, docs/dev/dsl/db-queries.md is the spec):
//! - [`encoding`]: order-preserving key codec + deterministic row codec;
//! - [`kv`]: the ordered-storage seam ([`kv::Kv`]) + host [`kv::MemKv`];
//! - [`spec`]: the QuerySpec value, canonical hash, opaque page tokens;
//! - [`engine`]: schema catalog, index-maintained writes, query execution.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

pub mod encoding;
pub mod engine;
pub mod kv;
pub mod spec;

pub use encoding::{QType, QVal};
pub use engine::{Engine, QueryError, TableDef};
pub use kv::{Kv, MemKv};
pub use spec::{Page, PageToken, QuerySpec, Range};
