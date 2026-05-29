// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

use alloc::vec::Vec;

/// A data buffer, optionally VMO-backed. Write-only from CPU, read-only from GPU.
#[derive(Debug, Clone)]
pub struct Buffer {
    pub(crate) data: Vec<u8>,
    #[allow(dead_code)]
    pub(crate) usage: crate::core::types::BufferUsage,
}

impl Buffer {
    /// Write data at offset. Pads with zeros if offset > len.
    pub fn write(&mut self, offset: usize, bytes: &[u8]) {
        let end = offset.saturating_add(bytes.len());
        if end > self.data.len() {
            self.data.resize(end, 0);
        }
        self.data[offset..end].copy_from_slice(bytes);
    }

    /// Read the full buffer contents.
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}
