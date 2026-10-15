// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: The board GPU's shader-core instruction encoding (TASK-0215 5c-2 P1, ADR-0072): an
//! instruction group — a header, up to six instructions in fixed phases, the source, select and
//! destination parts, padding to 16 bits — decoded into fields and encoded back, for the subset
//! the compositor's programs use. The bit positions are the hardware's interface as the open
//! userspace driver's compiler publishes it in its instruction description; this module is our
//! own encoder over those facts, proven by re-encoding captured programs byte for byte. The
//! backend that selects instructions from our IR writes through it.
//! OWNERS: @gpu @runtime
//! STATUS: Experimental (P1 the encoding and the setup programs; P3 the assembler, the reference
//!   programs the board measured, the backend for the compositor's textured layer; P4 the setup
//!   programs generated)
//! API_STABILITY: Unstable
//! TEST_COVERAGE: `tests/rogue_isa.rs` — the four programs of `resources/gpu/programs` decode
//!   into groups at the compiler's offsets and re-encode byte for byte; the rejects.
//!   `tests/rogue_pds.rs` — the bundle's setup programs word for word. `tests/rogue_asm.rs` —
//!   the compiler's groups rebuilt by the assembler. `tests/rogue_reference.rs` — the reference
//!   programs' groups and pass types. `tests/rogue_codegen.rs` — the IR's textured layer compiled
//!   to the references byte for byte; the backend's refusals. `tests/rogue_setup.rs` — the setup
//!   programs against the render bundle's; the setups the device refuses.
//!
//! - [`bits`] — a field as bit pieces over bytes, most significant first.
//! - [`header`] — the group header: main (brief or full), bitwise, control.
//! - [`io`] — the lower and upper sources, the internal-source selects, the destinations.
//! - [`alu`] — the main ALU's words.
//! - [`asm`] — groups from instructions and registers, the forms picked as the compiler picks them.
//! - [`backend`] — the backend's: mask save and move, visibility test, iterate, sample.
//! - [`bitwise`] — the bitwise unit's three phases.
//! - [`codegen`] — the backend: a fragment module of our IR compiled into groups.
//! - [`ctrl`] — the control instructions, iterate-and-sample among them.
//! - [`group`] — a group's parts in their order, its length, decode assist and padding.
//! - [`pds`] — the setup programs: kick, iterate, DMA; their constants' layouts.
//! - [`reference`] — hand-assembled programs the board measures and the backend must reproduce.
//! - [`setup`] — a fragment pipeline's setup programs (kick, iteration, state load) and state.

pub mod alu;
pub mod asm;
pub mod backend;
pub mod bits;
pub mod bitwise;
pub mod codegen;
pub mod ctrl;
pub mod group;
pub mod header;
pub mod io;
pub mod pds;
pub mod reference;
pub mod setup;

pub use group::{align_end, decode_program, encode_program, Group, Instr, ICACHE_ALIGN};

/// The phase an instruction executes in; phase 2's three main-ALU slots in their encoding
/// order's reverse (pack, test, move).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Phase {
    P0,
    P1,
    /// The bitwise unit's phase 2.
    P2,
    P2Pck,
    P2Tst,
    P2Mov,
    Backend,
    Ctrl,
}

/// Why bytes are not a group of this subset, or a group not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RogueError {
    /// The bytes end inside a part (`at`: the part's offset in the program).
    Truncated { at: usize },
    /// A reserved or fixed field holds another value.
    Reserved { at: usize, what: &'static str },
    /// The parts do not fit the group's own description.
    Malformed { at: usize, what: &'static str },
    /// An encoding outside this subset.
    Unsupported { at: usize, what: &'static str },
    /// A value wider than its field, or a group longer than the header can name.
    TooWide { what: &'static str, value: u32 },
}
