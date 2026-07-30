use std::time::Duration;

use crate::services::news::backoff::RetryBackoff;

#[test]
fn deterministic_sequence_caps_and_retry_after_never_shortens_it() {
    let mut backoff = RetryBackoff::default();
    let inputs = [
        None,
        None,
        Some(Duration::from_secs(2)),
        Some(Duration::from_secs(40)),
        None,
        Some(Duration::from_secs(90)),
        None,
    ];
    let actual: Vec<_> = inputs
        .into_iter()
        .map(|retry_after| backoff.next_delay(retry_after))
        .collect();

    assert_eq!(actual, [3, 6, 12, 40, 48, 60, 60].map(Duration::from_secs));
}

#[test]
fn successful_response_resets_next_delay_to_three_seconds() {
    let mut backoff = RetryBackoff::default();
    assert_eq!(backoff.next_delay(None), Duration::from_secs(3));
    assert_eq!(backoff.next_delay(None), Duration::from_secs(6));

    backoff.reset();

    assert_eq!(backoff.next_delay(None), Duration::from_secs(3));
}
