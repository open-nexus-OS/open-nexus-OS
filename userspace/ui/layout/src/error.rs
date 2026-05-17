// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//!
//! CONTEXT: Layout errors for TASK-0058 / RFC-0057.
//! OWNERS: @ui
//! STATUS: Done
//! ADR: docs/rfcs/RFC-0057-ui-v3a-layout-engine-pretext-contract.md

use core::fmt;

/// Errors that can occur during layout computation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutError {
    /// The layout tree exceeds the maximum allowed node count.
    TooManyNodes { max: usize, actual: usize },
    /// The layout tree exceeds the maximum nesting depth.
    TooDeep { max: usize, actual: usize },
    /// Text measurement failed.
    MeasureFailed,
    /// Division by zero in layout math (e.g. flex fraction with zero total weight).
    DivByZero,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyNodes { max, actual } => {
                write!(f, "too many layout nodes: max {max}, got {actual}")
            }
            Self::TooDeep { max, actual } => {
                write!(f, "layout tree too deep: max depth {max}, got {actual}")
            }
            Self::MeasureFailed => write!(f, "text measurement failed"),
            Self::DivByZero => write!(f, "division by zero in layout math"),
        }
    }
}
