// Copyright 2026 Open Nexus OS Contributors
// SPDX-License-Identifier: Apache-2.0
//
//! CONTEXT: Narrow `inputd` route target seam preserving `windowd` as authority.
//! OWNERS: @runtime
//! STATUS: Experimental
//! API_STABILITY: Unstable
//! TEST_COVERAGE: 8 host contract tests in the `inputd` crate.
//! ADR: docs/adr/0029-input-v1-host-core-architecture.md

pub trait RouteTarget {
    fn route_pointer_move(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery>;
    fn route_pointer_down(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery>;
    fn route_keyboard(&mut self, key_code: u32) -> windowd::Result<windowd::InputDelivery>;
    fn route_touch(
        &mut self,
        x: i32,
        y: i32,
        phase: windowd::TouchInputPhase,
    ) -> windowd::Result<windowd::InputDelivery>;
    fn bounds(&self) -> (u32, u32);
}

impl RouteTarget for windowd::WindowServer {
    fn route_pointer_move(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_pointer_move(self, x, y)
    }

    fn route_pointer_down(&mut self, x: i32, y: i32) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_pointer_down(self, x, y)
    }

    fn route_keyboard(&mut self, key_code: u32) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_keyboard(self, key_code)
    }

    fn route_touch(
        &mut self,
        x: i32,
        y: i32,
        phase: windowd::TouchInputPhase,
    ) -> windowd::Result<windowd::InputDelivery> {
        windowd::WindowServer::route_touch(self, x, y, phase)
    }

    fn bounds(&self) -> (u32, u32) {
        let config = self.config();
        (config.width, config.height)
    }
}
