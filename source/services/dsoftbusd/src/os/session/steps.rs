//! Pure orchestration step helpers for runner-near tests.

use super::fsm::SessionFsm;

#[inline]
pub(crate) fn should_send_announce(announced_once: bool, now_tick: u64) -> bool {
    !announced_once || (now_tick & 0x3f) == 0
}

#[inline]
pub(crate) fn should_poll_discovery(peer_known: bool, now_tick: u64) -> bool {
    !peer_known || (now_tick & 0x1f) == 0
}

#[inline]
pub(crate) fn identity_binding_matches(
    discovered_noise_static: Option<[u8; 32]>,
    expected_noise_static: [u8; 32],
) -> bool {
    // Preserve existing behavior: if discovery mapping is absent, do not fail hard here.
    discovered_noise_static.map(|v| v == expected_noise_static).unwrap_or(true)
}

pub(crate) struct HandshakeFailureAction<S> {
    pub(crate) close_sid: Option<S>,
    pub(crate) retry: bool,
}

#[inline]
pub(crate) fn on_handshake_failure<S: Copy>(fsm: &mut SessionFsm<S>) -> HandshakeFailureAction<S> {
    HandshakeFailureAction { close_sid: fsm.begin_reconnect(), retry: true }
}
