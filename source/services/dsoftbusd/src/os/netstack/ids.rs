//! Typed wrappers for netstack socket/listener/session handles.

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct UdpSocketId(u32);

impl UdpSocketId {
    #[inline]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) fn as_raw(self) -> u32 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ListenerId(u32);

impl ListenerId {
    #[inline]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) fn as_raw(self) -> u32 {
        self.0
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct SessionId(u32);

impl SessionId {
    #[inline]
    pub(crate) fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    #[inline]
    pub(crate) fn as_raw(self) -> u32 {
        self.0
    }
}
