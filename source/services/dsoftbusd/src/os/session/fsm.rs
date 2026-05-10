//! Session phase + epoch ownership state machine.

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct EpochId(u32);

impl EpochId {
    #[inline]
    pub(crate) fn initial() -> Self {
        Self(1)
    }

    #[inline]
    pub(crate) fn next(self) -> Self {
        Self(self.0.wrapping_add(1))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionPhase {
    Idle,
    Listening,
    Dialing,
    Accepting,
    Connected,
    Handshaking,
    Ready,
    Reconnect,
}

pub(crate) struct SessionFsm<S> {
    phase: SessionPhase,
    epoch: EpochId,
    sid: Option<S>,
}

impl<S: Copy> SessionFsm<S> {
    #[inline]
    pub(crate) fn new() -> Self {
        Self {
            phase: SessionPhase::Idle,
            epoch: EpochId::initial(),
            sid: None,
        }
    }

    #[inline]
    pub(crate) fn set_listening(&mut self) {
        self.phase = SessionPhase::Listening;
    }

    #[inline]
    pub(crate) fn set_dialing(&mut self) {
        self.phase = SessionPhase::Dialing;
    }

    #[inline]
    pub(crate) fn set_accepting(&mut self) {
        self.phase = SessionPhase::Accepting;
    }

    #[inline]
    pub(crate) fn set_connected(&mut self, sid: S) {
        self.sid = Some(sid);
        self.phase = SessionPhase::Connected;
    }

    #[inline]
    pub(crate) fn set_handshaking(&mut self) {
        self.phase = SessionPhase::Handshaking;
    }

    #[inline]
    pub(crate) fn set_ready(&mut self) {
        self.phase = SessionPhase::Ready;
    }

    #[inline]
    pub(crate) fn begin_reconnect(&mut self) -> Option<S> {
        self.phase = SessionPhase::Reconnect;
        let old = self.sid.take();
        self.epoch = self.epoch.next();
        old
    }

    #[inline]
    pub(crate) fn sid(&self) -> Option<S> {
        self.sid
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn epoch_raw(&self) -> u32 {
        self.epoch.0
    }

    #[cfg(test)]
    #[inline]
    pub(crate) fn phase(&self) -> SessionPhase {
        self.phase
    }
}
