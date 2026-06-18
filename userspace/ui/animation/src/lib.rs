// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

#![allow(clippy::unwrap_used)]

//! CONTEXT: Animation engine: timeline, spring physics (RK4), keyframe interpolation.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

#![cfg_attr(target_os = "none", no_std)]

extern crate alloc;

pub mod keyframe;
pub mod property;
pub mod scroll;
pub mod spring;
pub mod timeline;

pub use keyframe::{Easing, KeyframeTrack};
pub use property::{AnimProp, LayerId, SceneUpdate};
pub use scroll::{ScrollConfig, ScrollMomentum};
pub use spring::{SpringConfig, SpringSim};
pub use timeline::AnimationDriver;
