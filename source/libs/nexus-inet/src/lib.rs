// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: nexus-inet — minimal interaction-combinator net evaluator
//! (SMP track Phase E; the exchangeable v2 compute backend behind pinched's
//! job-graph interface). Agents are Lafont-style interaction combinators
//! (ERA/CON/DUP) plus a number leaf and a binary-add operator so real batch
//! workloads (tree reductions) evaluate to values. Interactions are LOCAL
//! (they touch only the two redex nodes and the peers of their ports), and
//! the calculus is confluent — the normal form is independent of reduction
//! order, which makes `workers = 1 ≡ workers = N` hold BY CONSTRUCTION.
//! Reduction is round-based: each round's redex list is partitioned across
//! workers (deterministic chunking); ports are atomics, arena alloc/free and
//! the next-round queue sit behind plain spin locks (TASK-0277: no lock-free
//! experiments in v1 — atomic port-linking races are a later, separately
//! proven step).
//! OWNERS: @runtime
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: host tests (tree-sum value, equality matrix with real
//!   std threads, bounded arena reject); QEMU markers `SELFTEST: inet
//!   determinism/bounded/parallel exec ok` via the pinched job kind.
//! PUBLIC API: Arena, NodeKind, Redex, build_tree_sum(), reduce_chunk(),
//!   reduce_to_normal_form(), root_value()
//! DEPENDS_ON: nexus-sync
//! ADR: docs/adr/0047-interaction-net-evaluator-backend.md

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]

extern crate alloc;

mod arena;
mod reduce;

pub use arena::{Arena, InetError, NodeKind, Port, Redex, NIL};
pub use reduce::{build_tree_sum, reduce_chunk, reduce_to_normal_form, root_value, RoundOut};
