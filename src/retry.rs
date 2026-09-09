use std::time::Duration;

pub const INITIAL_RECONNECT_DELAY: Duration = Duration::from_millis(200);
pub const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(5);
pub const PRE_RELAY_RECONNECT_WINDOW: Duration = Duration::from_mins(1);

#[derive(Clone, Debug)]
pub struct ReconnectBackoff {
    attempt: u32,
}

impl ReconnectBackoff {
    #[must_use]
    pub const fn new() -> Self {
        Self { attempt: 0 }
    }

    #[must_use]
    pub fn next_delay_with(&mut self, jitter: impl FnOnce(Duration) -> Duration) -> Duration {
        let exponent = self.attempt.min(5);
        self.attempt = self.attempt.saturating_add(1);
        let cap = INITIAL_RECONNECT_DELAY
            .checked_mul(1_u32 << exponent)
            .unwrap_or(MAX_RECONNECT_DELAY)
            .min(MAX_RECONNECT_DELAY);
        jitter(cap).min(cap)
    }

    #[must_use]
    pub fn next_delay(&mut self) -> Duration {
        self.next_delay_with(|cap| {
            let cap_nanos = u64::try_from(cap.as_nanos()).unwrap_or(u64::MAX);
            Duration::from_nanos(fastrand::u64(..=cap_nanos))
        })
    }
}

impl Default for ReconnectBackoff {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_and_allows_controlled_full_jitter() {
        let mut backoff = ReconnectBackoff::new();
        assert_eq!(backoff.next_delay_with(|_| Duration::ZERO), Duration::ZERO);
        assert_eq!(
            backoff.next_delay_with(|cap| cap),
            Duration::from_millis(400)
        );
        for _ in 0..10 {
            assert!(backoff.next_delay_with(|cap| cap) <= MAX_RECONNECT_DELAY);
        }
    }

    #[test]
    fn every_session_starts_with_initial_cap() {
        let mut first = ReconnectBackoff::new();
        let _ = first.next_delay_with(|cap| cap);
        let mut next_session = ReconnectBackoff::new();
        assert_eq!(
            next_session.next_delay_with(|cap| cap),
            INITIAL_RECONNECT_DELAY
        );
    }
}
