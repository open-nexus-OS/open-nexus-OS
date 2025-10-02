#![cfg_attr(not(test), no_std)]

/// Simple scheduler timeline helper for user space services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Deadline {
    pub ticks: u64,
}

impl Deadline {
    pub const fn from_ms(ms: u64) -> Self {
        Self { ticks: ms * 1_000 }
    }

    pub fn expired(self, now: u64) -> bool {
        now >= self.ticks
    }
}

#[cfg(test)]
mod tests {
    use super::Deadline;

    #[test]
    fn expiry_checks() {
        let deadline = Deadline::from_ms(10);
        assert!(!deadline.expired(5_000));
        assert!(deadline.expired(10_000));
    }
}
