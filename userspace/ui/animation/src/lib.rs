// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! CONTEXT: Animation engine: timeline, spring physics (RK4), keyframe interpolation.
//! OWNERS: @ui
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! RFC: docs/rfcs/RFC-0059-ui-v5a-animation-nexusgfx-sdk-gpu-driver-contract.md

pub mod timeline;
pub mod spring;
pub mod keyframe;
pub mod property;

pub use timeline::AnimationDriver;
pub use spring::{SpringConfig, SpringSim};
pub use keyframe::{Easing, KeyframeTrack};
pub use property::{AnimProp, LayerId, SceneUpdate};
