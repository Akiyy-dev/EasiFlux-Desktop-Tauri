use std::time::Duration;

const RETRY_SECONDS: [u64; 6] = [3, 6, 12, 24, 48, 60];

#[derive(Debug, Default)]
pub(super) struct RetryBackoff {
    attempt: usize,
}

impl RetryBackoff {
    pub(super) fn next_delay(&mut self, retry_after: Option<Duration>) -> Duration {
        let sequence = Duration::from_secs(
            RETRY_SECONDS[self.attempt.min(RETRY_SECONDS.len().saturating_sub(1))],
        );
        self.attempt = self.attempt.saturating_add(1);
        retry_after
            .unwrap_or_default()
            .max(sequence)
            .min(Duration::from_secs(60))
    }

    pub(super) fn reset(&mut self) {
        self.attempt = 0;
    }
}
