// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! A context's pipelines on the board GPU (TASK-0215 5c-2 P4d, RFC-0101 §7b): the pipeline
//! library a client supplies, parsed for this device and each accepted entry checked again for
//! it — its program decodes into instruction groups within its budget, its setup decodes for its
//! interface — then its program placed in the client's code heap and its setup programs in its
//! setup heap, the kick naming where the program landed. The device patches no program byte;
//! only the kick receives an address. A draw names a pipeline by id, and an id the context did
//! not accept is refused. A context takes one library.

use alloc::string::String;
use alloc::vec::Vec;

use super::csb::ppp;
use crate::shader::ir::Stage;
use crate::shader::library::{self, LibraryError, Refusal};
use crate::shader::rogue::decode_program;
use crate::shader::rogue::setup::Setup;

/// Where a pipeline's bytes go: the client's code heap and setup heap.
pub trait Heaps {
    /// Writes a program where the code heap has room: its offset from the heap's base.
    fn code(&mut self, bytes: &[u8]) -> Option<u32>;
    /// Writes a setup program where the setup heap has room (16-byte aligned): its offset.
    fn setup(&mut self, bytes: &[u8]) -> Option<u32>;
}

/// A pipeline placed in a context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placed {
    pub id: u32,
    pub name: String,
    pub groups: u16,
    pub pass_type: u8,
    /// The program's offset in the code heap (the kick names it).
    pub exe_off: u32,
    /// Setup heap offsets: the kick, the varying iteration, the state load's code.
    pub kick: u32,
    pub iterate: u32,
    pub state_code: u32,
    pub setup: Setup,
}

impl Placed {
    /// The draw state's PDS words for a draw whose state-load data sits at `state_data`.
    pub fn pds_state(&self, state_data: u32) -> ppp::PdsState {
        let (sizeinfo1, sizeinfo2) = self.setup.sizeinfo();
        ppp::PdsState {
            shaderbase: self.kick,
            texunicodebase: self.state_code,
            varyingbase: self.iterate,
            uniformdatabase: state_data,
            sizeinfo1,
            sizeinfo2,
        }
    }
}

/// Why a library is not taken or a pipeline not found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableError {
    Library(LibraryError),
    /// The context already holds its library.
    Loaded,
    /// A heap has no room for an accepted entry.
    NoRoom,
    /// A draw named an id this context did not accept.
    UnknownPipeline(u32),
}

/// Why an entry the library accepted is refused on this device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    Library(Refusal),
    /// The program does not decode into this GPU's instruction groups.
    Program,
    /// More groups than the entry's budget.
    OverBudget,
    /// The setup programs or state are not what this device runs.
    Setup,
}

/// What a load took.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loaded {
    pub accepted: Vec<u32>,
    pub refused: Vec<(u32, Refused)>,
    pub code_bytes: usize,
    pub setup_bytes: usize,
}

/// One context's pipelines.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PipelineTable {
    placed: Vec<Placed>,
}

fn place_setup(heaps: &mut dyn Heaps, bytes: &[u8], total: &mut usize) -> Result<u32, TableError> {
    *total += bytes.len();
    heaps.setup(bytes).ok_or(TableError::NoRoom)
}

impl PipelineTable {
    /// Takes the context's library: `target` and `features` are the device's.
    pub fn load(
        &mut self,
        bytes: &[u8],
        target: u64,
        features: u32,
        heaps: &mut dyn Heaps,
    ) -> Result<Loaded, TableError> {
        if !self.placed.is_empty() {
            return Err(TableError::Loaded);
        }
        let lib = library::parse(bytes, target, features).map_err(TableError::Library)?;
        let mut out = Loaded {
            refused: lib.refused.iter().map(|(id, r)| (*id, Refused::Library(*r))).collect(),
            ..Loaded::default()
        };
        let mut placed = Vec::with_capacity(lib.accepted.len());
        for e in &lib.accepted {
            let Ok(groups) = decode_program(e.program) else {
                out.refused.push((e.id, Refused::Program));
                continue;
            };
            if groups.len() > usize::from(e.budget) {
                out.refused.push((e.id, Refused::OverBudget));
                continue;
            }
            let setup = match Setup::from_sections(&e.setup, e.state, &e.interface) {
                Ok(s) if e.interface.stage == Stage::Fragment => s,
                _ => {
                    out.refused.push((e.id, Refused::Setup));
                    continue;
                }
            };
            let exe_off = heaps.code(e.program).ok_or(TableError::NoRoom)?;
            out.code_bytes += e.program.len();
            let encode = |r: Result<Vec<u8>, _>| r.map_err(|_| TableError::NoRoom);
            let kick = encode(setup.kick_at(exe_off).and_then(|k| k.encode()))?;
            let kick = place_setup(heaps, &kick, &mut out.setup_bytes)?;
            let iterate =
                place_setup(heaps, &encode(setup.iterate.encode())?, &mut out.setup_bytes)?;
            let mut code = Vec::with_capacity(4 * setup.state_code.len());
            for i in &setup.state_code {
                code.extend_from_slice(&i.encode().map_err(|_| TableError::NoRoom)?.to_le_bytes());
            }
            let state_code = place_setup(heaps, &code, &mut out.setup_bytes)?;
            out.accepted.push(e.id);
            placed.push(Placed {
                id: e.id,
                name: String::from(e.name),
                groups: groups.len() as u16,
                pass_type: setup.state.pass_type,
                exe_off,
                kick,
                iterate,
                state_code,
                setup,
            });
        }
        self.placed = placed;
        Ok(out)
    }

    /// The pipeline a draw names.
    pub fn get(&self, id: u32) -> Result<&Placed, TableError> {
        self.placed.iter().find(|p| p.id == id).ok_or(TableError::UnknownPipeline(id))
    }

    pub fn ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.placed.iter().map(|p| p.id)
    }

    /// The id the client's library gave a name (a client knows its own library's names).
    pub fn id_of(&self, name: &str) -> Option<u32> {
        self.placed.iter().find(|p| p.name == name).map(|p| p.id)
    }
}
