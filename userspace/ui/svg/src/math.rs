// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0

//! Math trait bridging std and no_std f32 operations.

#[cfg(feature = "std")]
mod imp {
    pub trait F32Math: Sized {
        fn nexus_sqrt(self) -> Self;
        fn nexus_cos(self) -> Self;
        fn nexus_sin(self) -> Self;
        fn nexus_sin_cos(self) -> (Self, Self);
        fn nexus_to_radians(self) -> Self;
    }
    impl F32Math for f32 {
        fn nexus_sqrt(self) -> Self {
            self.sqrt()
        }
        fn nexus_cos(self) -> Self {
            self.cos()
        }
        fn nexus_sin(self) -> Self {
            self.sin()
        }
        fn nexus_sin_cos(self) -> (Self, Self) {
            self.sin_cos()
        }
        fn nexus_to_radians(self) -> Self {
            self.to_radians()
        }
    }
}

#[cfg(not(feature = "std"))]
mod imp {
    pub trait F32Math: Sized {
        fn nexus_sqrt(self) -> Self;
        fn nexus_cos(self) -> Self;
        fn nexus_sin(self) -> Self;
        fn nexus_sin_cos(self) -> (Self, Self);
        fn nexus_to_radians(self) -> Self;
    }
    impl F32Math for f32 {
        fn nexus_sqrt(self) -> Self {
            libm::sqrtf(self)
        }
        fn nexus_cos(self) -> Self {
            libm::cosf(self)
        }
        fn nexus_sin(self) -> Self {
            libm::sinf(self)
        }
        fn nexus_sin_cos(self) -> (Self, Self) {
            (libm::sinf(self), libm::cosf(self))
        }
        fn nexus_to_radians(self) -> Self {
            self * core::f32::consts::PI / 180.0
        }
    }
}

pub(crate) use imp::F32Math;
