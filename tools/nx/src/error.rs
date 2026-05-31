// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Shared exit classes and error result types for the canonical `nx` CLI.
//! OWNERS: @tools-team
//! STATUS: Functional
//! API_STABILITY: Unstable
//! TEST_COVERAGE: Covered by `nx` command tests.
//! ADR: docs/adr/0021-structured-data-formats-json-vs-capnp.md

use serde_json::Value;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    Success,
    Usage,
    ValidationReject,
    MissingDependency,
    DelegateFailure,
    Unsupported,
    Internal,
}

impl ExitClass {
    pub fn code(self) -> i32 {
        match self {
            Self::Success => 0,
            Self::Usage => 2,
            Self::ValidationReject => 3,
            Self::MissingDependency => 4,
            Self::DelegateFailure => 5,
            Self::Unsupported => 6,
            Self::Internal => 7,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Usage => "usage",
            Self::ValidationReject => "validation_reject",
            Self::MissingDependency => "missing_dependency",
            Self::DelegateFailure => "delegate_failure",
            Self::Unsupported => "unsupported",
            Self::Internal => "internal",
        }
    }
}

#[derive(Debug)]
pub struct NxError {
    pub(crate) class: ExitClass,
    pub(crate) message: String,
}

impl NxError {
    pub(crate) fn new(class: ExitClass, message: impl Into<String>) -> Self {
        Self { class, message: message.into() }
    }
}

impl fmt::Display for NxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for NxError {}

pub(crate) type ExecResult = Result<(ExitClass, String, bool, Option<Value>), NxError>;
