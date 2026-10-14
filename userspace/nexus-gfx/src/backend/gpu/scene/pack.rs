// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! The packer's half of the scene bundle (TASK-0169B P1): the sparse non-zero runs of a
//! captured buffer and the builder that writes a bundle in the format [`super`] parses.

use alloc::vec::Vec;

use super::{
    align4, align8, CoreId, FreelistArgs, HwrtArgs, JobKind, Kind, Texture, HEADER_BYTES, MAGIC,
    VERSION, VERSION_RENDER,
};

/// Non-zero runs of `bytes`, gaps shorter than `merge` folded into the run.
pub fn runs_of(bytes: &[u8], merge: usize) -> Vec<(u32, Vec<u8>)> {
    let mut runs: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0 {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i + 1;
        loop {
            let mut j = end;
            while j < bytes.len() && bytes[j] == 0 {
                j += 1;
            }
            if j < bytes.len() && j - end < merge {
                end = j + 1;
                while end < bytes.len() && bytes[end] != 0 {
                    end += 1;
                }
            } else {
                break;
            }
        }
        runs.push((start as u32, bytes[start..end].to_vec()));
        i = end;
    }
    runs
}

/// Builds a bundle (the packer's half).
pub struct Builder {
    bytes: Vec<u8>,
    jobs: Vec<u8>,
    buffer_count: u32,
    job_count: u32,
    /// The render trailer (version 2), once given.
    render: Option<Vec<u8>>,
}

impl Builder {
    pub fn new(core: CoreId, texture: Texture) -> Self {
        let mut bytes = Vec::with_capacity(HEADER_BYTES);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
        bytes.extend_from_slice(&core.0.to_le_bytes());
        bytes.extend_from_slice(&[0u8; 8]); // counts, filled at finish
        bytes.extend_from_slice(&(texture as u32).to_le_bytes());
        bytes.extend_from_slice(&0u32.to_le_bytes());
        Self { bytes, jobs: Vec::new(), buffer_count: 0, job_count: 0, render: None }
    }

    /// The render pass's kernel-side objects: the dataset's 136-byte arguments, the free lists
    /// `[local, global]`, the render context's 192-byte static state stream. Makes the bundle a
    /// version-2 one.
    pub fn render(
        &mut self,
        hwrt_args: &[u8],
        callstack_addr: u64,
        freelists: &[FreelistArgs; 2],
        static_state: &[u8],
    ) {
        let mut t = Vec::with_capacity(HwrtArgs::SIZE + 8 + 4 + 48 + 4 + static_state.len());
        t.extend_from_slice(&hwrt_args[..HwrtArgs::SIZE.min(hwrt_args.len())]);
        t.resize(HwrtArgs::SIZE, 0);
        t.extend_from_slice(&callstack_addr.to_le_bytes());
        t.extend_from_slice(&2u32.to_le_bytes());
        for fl in freelists {
            t.extend_from_slice(&fl.gpu_addr.to_le_bytes());
            t.extend_from_slice(&fl.initial_pages.to_le_bytes());
            t.extend_from_slice(&fl.max_pages.to_le_bytes());
            t.extend_from_slice(&fl.grow_pages.to_le_bytes());
            t.extend_from_slice(&fl.grow_threshold_percent.to_le_bytes());
        }
        t.extend_from_slice(&(static_state.len() as u32).to_le_bytes());
        t.extend_from_slice(static_state);
        t.resize(align8(t.len()), 0);
        self.render = Some(t);
    }

    /// A buffer with its runs (offset, bytes).
    pub fn buffer(&mut self, va: u64, len: u64, kind: Kind, runs: &[(u32, Vec<u8>)]) {
        self.bytes.extend_from_slice(&va.to_le_bytes());
        self.bytes.extend_from_slice(&len.to_le_bytes());
        self.bytes.extend_from_slice(&(kind as u32).to_le_bytes());
        self.bytes.extend_from_slice(&(runs.len() as u32).to_le_bytes());
        for (off, data) in runs {
            self.bytes.extend_from_slice(&off.to_le_bytes());
            self.bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
            self.bytes.extend_from_slice(data);
            self.bytes.resize(align4(self.bytes.len()), 0);
        }
        self.buffer_count += 1;
    }

    pub fn job(&mut self, kind: JobKind, flags: u32, stream: &[u8]) {
        self.jobs.extend_from_slice(&(kind as u32).to_le_bytes());
        self.jobs.extend_from_slice(&flags.to_le_bytes());
        self.jobs.extend_from_slice(&(stream.len() as u32).to_le_bytes());
        self.jobs.extend_from_slice(&0u32.to_le_bytes());
        self.jobs.extend_from_slice(stream);
        self.jobs.resize(align8(self.jobs.len()), 0);
        self.job_count += 1;
    }

    pub fn finish(mut self) -> Vec<u8> {
        let version = if self.render.is_some() { VERSION_RENDER } else { VERSION };
        self.bytes[4..8].copy_from_slice(&version.to_le_bytes());
        self.bytes[16..20].copy_from_slice(&self.buffer_count.to_le_bytes());
        self.bytes[20..24].copy_from_slice(&self.job_count.to_le_bytes());
        self.bytes.extend_from_slice(&self.jobs);
        if let Some(t) = self.render.take() {
            self.bytes.extend_from_slice(&t);
        }
        self.bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_fold_short_gaps_and_keep_long_ones() {
        let mut bytes = alloc::vec![0u8; 200];
        bytes[10] = 1;
        bytes[12] = 2; // gap of 1 folded
        bytes[100] = 3; // gap of 87 kept
        let runs = runs_of(&bytes, 32);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], (10, alloc::vec![1, 0, 2]));
        assert_eq!(runs[1], (100, alloc::vec![3]));
    }
}
