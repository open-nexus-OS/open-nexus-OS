// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pipeline timing gates — deterministic performance measurement for the
//! render pipeline. Every stage (compose, IPC, present) records wall-clock
//! duration. Soll-gate assertions reject frames that exceed budget.
//!
//! Design constraints:
//! - Zero heap allocation after construction (pre-allocated ring buffer)
//! - Monotonic nanosecond counter (no wall-clock dependency)
//! - Host-first: gates run on x86_64 host tests, verified on RISC-V QEMU

use alloc::vec::Vec;

/// Maximum number of frame samples retained in the ring buffer.
pub const MAX_FRAME_SAMPLES: usize = 256;

/// A single frame's timing record.
#[derive(Debug, Clone, Copy)]
pub struct FrameSample {
    /// Total frame wall time (compose + IPC + present).
    pub total_ns: u64,
    /// CPU compositing time (blur, SDF, shadow, vmo_write).
    pub compose_ns: u64,
    /// IPC round-trip time (send + recv).
    pub ipc_ns: u64,
    /// GPU/backend present time (TRANSFER_TO_HOST + FLUSH).
    pub present_ns: u64,
    /// Number of damage rects in this frame.
    pub damage_rects: u16,
    /// Total pixels transferred in this frame (damage area × 4).
    pub transfer_bytes: u64,
}

impl Default for FrameSample {
    fn default() -> Self {
        Self { total_ns: 0, compose_ns: 0, ipc_ns: 0, present_ns: 0, damage_rects: 0, transfer_bytes: 0 }
    }
}

/// Soll-gate definition — a hard performance requirement.
#[derive(Debug, Clone, Copy)]
pub struct SollGate {
    /// Gate name for diagnostics.
    pub name: &'static str,
    /// Maximum allowed value. Frames exceeding this fail the gate.
    pub max_ns: u64,
}

/// Collection of Soll-gates that must all pass.
pub const SOLL_GATES: &[SollGate] = &[
    SollGate { name: "frame_total_120hz", max_ns: 8_333_333 },
    SollGate { name: "compose_120hz", max_ns: 6_000_000 },
    SollGate { name: "ipc_latency", max_ns: 100_000 },
    SollGate { name: "present_dma", max_ns: 2_000_000 },
];

/// Ring-buffer pipeline timer. Records frame samples and validates
/// against Soll-gates. All operations are infallible after construction.
pub struct PipelineTimer {
    samples: Vec<FrameSample>,
    cursor: usize,
    frame_count: u64,
    /// Current frame being assembled.
    current: FrameSample,
    stage_start_ns: u64,
}

impl PipelineTimer {
    /// Create a new timer with pre-allocated ring buffer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            samples: Vec::with_capacity(MAX_FRAME_SAMPLES),
            cursor: 0,
            frame_count: 0,
            current: FrameSample::default(),
            stage_start_ns: 0,
        }
    }

    /// Begin timing a new frame. Resets the current sample.
    pub fn begin_frame(&mut self, now_ns: u64) {
        self.current = FrameSample::default();
        self.stage_start_ns = now_ns;
        self.current.total_ns = now_ns; // will be replaced at end_frame
    }

    /// Begin timing a stage within the current frame.
    pub fn begin_stage(&mut self, now_ns: u64) {
        self.stage_start_ns = now_ns;
    }

    /// End the compose stage. Records elapsed time.
    pub fn end_compose(&mut self, now_ns: u64, damage_rects: u16, transfer_bytes: u64) {
        self.current.compose_ns = now_ns.saturating_sub(self.stage_start_ns);
        self.current.damage_rects = damage_rects;
        self.current.transfer_bytes = transfer_bytes;
        self.stage_start_ns = now_ns;
    }

    /// End the IPC stage. Records elapsed time.
    pub fn end_ipc(&mut self, now_ns: u64) {
        self.current.ipc_ns = now_ns.saturating_sub(self.stage_start_ns);
        self.stage_start_ns = now_ns;
    }

    /// End the present stage. Records elapsed time.
    pub fn end_present(&mut self, now_ns: u64) {
        self.current.present_ns = now_ns.saturating_sub(self.stage_start_ns);
        self.stage_start_ns = now_ns;
    }

    /// End the current frame. Computes total time and stores the sample.
    pub fn end_frame(&mut self, now_ns: u64) {
        self.current.total_ns = now_ns.saturating_sub(self.current.total_ns);
        self.frame_count = self.frame_count.wrapping_add(1);
        if self.samples.len() < MAX_FRAME_SAMPLES {
            self.samples.push(self.current);
        } else {
            self.samples[self.cursor] = self.current;
            self.cursor = (self.cursor + 1) % MAX_FRAME_SAMPLES;
        }
    }

    /// Return the most recent frame sample, if any.
    #[must_use]
    pub fn last_sample(&self) -> Option<FrameSample> {
        if self.samples.is_empty() {
            None
        } else if self.samples.len() < MAX_FRAME_SAMPLES {
            self.samples.last().copied()
        } else {
            let idx = self.cursor.checked_sub(1).unwrap_or(MAX_FRAME_SAMPLES - 1);
            Some(self.samples[idx])
        }
    }

    /// Total frames recorded.
    #[must_use]
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Validate all samples against all Soll-gates.
    /// Returns the first failing gate + sample, or None if all pass.
    #[must_use]
    pub fn validate_gates(&self) -> Option<(&'static str, u64, u64)> {
        for sample in &self.samples {
            for gate in SOLL_GATES {
                let value = match gate.name {
                    "frame_total_120hz" => sample.total_ns,
                    "compose_120hz" => sample.compose_ns,
                    "ipc_latency" => sample.ipc_ns,
                    "present_dma" => sample.present_ns,
                    _ => continue,
                };
                if value > gate.max_ns {
                    return Some((gate.name, value, gate.max_ns));
                }
            }
        }
        None
    }

    /// Compute p50, p95, p99 of total frame times from all samples.
    #[must_use]
    pub fn percentiles(&self) -> (u64, u64, u64) {
        if self.samples.is_empty() {
            return (0, 0, 0);
        }
        let mut times: Vec<u64> = self.samples.iter().map(|s| s.total_ns).collect();
        times.sort_unstable();
        let len = times.len();
        let p50 = times[len / 2];
        let p95 = times[(len * 95) / 100];
        let p99 = times[(len * 99) / 100];
        (p50, p95, p99)
    }
}

impl Default for PipelineTimer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_records_frame_samples() {
        let mut t = PipelineTimer::new();
        t.begin_frame(0);
        t.begin_stage(100);
        t.end_compose(1_000_000, 2, 500_000);
        t.begin_stage(1_000_000);
        t.end_ipc(1_050_000);
        t.begin_stage(1_050_000);
        t.end_present(2_000_000);
        t.end_frame(2_100_000);

        let s = t.last_sample().unwrap();
        assert_eq!(s.compose_ns, 900_000);
        assert_eq!(s.ipc_ns, 50_000);
        assert_eq!(s.present_ns, 950_000);
        assert_eq!(s.damage_rects, 2);
        assert_eq!(s.transfer_bytes, 500_000);
    }

    #[test]
    fn soll_gates_detect_violation() {
        let mut t = PipelineTimer::new();
        t.begin_frame(0);
        t.begin_stage(0);
        t.end_compose(10_000_000, 1, 100); // exceeds 6ms compose gate
        t.begin_stage(10_000_000);
        t.end_ipc(10_000_000);
        t.begin_stage(10_000_000);
        t.end_present(10_000_000);
        t.end_frame(10_000_000);

        let violation = t.validate_gates();
        assert!(violation.is_some());
        let (name, value, max) = violation.unwrap();
        assert_eq!(name, "compose_120hz");
        assert!(value > max);
    }

    #[test]
    fn empty_timer_passes_all_gates() {
        let t = PipelineTimer::new();
        assert!(t.validate_gates().is_none());
    }

    #[test]
    fn percentiles_compute_correctly() {
        let mut t = PipelineTimer::new();
        for i in 0..10 {
            t.begin_frame(0);
            t.begin_stage(0);
            t.end_compose(0, 0, 0);
            t.begin_stage(0);
            t.end_ipc(0);
            t.begin_stage(0);
            t.end_present((i * 100_000) as u64);
            t.end_frame((i * 100_000) as u64);
        }
        let (p50, _p95, _p99) = t.percentiles();
        assert!(p50 > 0);
    }
}
