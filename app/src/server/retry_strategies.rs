use std::fmt::Display;
use std::future::Future;
use std::time::Duration;

use anyhow::Result;
use torchui::RetryOption;

/// Common duration for a periodic poll. In our app, we generally have the following to update the same data:
/// - RTC messages
/// - Out-of-band queries based on user actions (i.e. fetch team info when user opens the settings page, user
/// starts the app)
/// However, we also periodically poll for updates in case RTC is down, the user's websocket
/// is borked, etc.
/// For team memberships, we also don't yet process messages for joining or leaving a team, so the user would see these
/// updates only after a periodic poll.
pub const PERIODIC_POLL: Duration = Duration::from_secs(60 * 10);

/// For a periodic poll, it's fine to wait for longer period of time between retries. However, we don't want this to be so
/// long that it's around the same as the overall periodic poll interval.
pub const PERIODIC_POLL_RETRY_STRATEGY: RetryOption = RetryOption::exponential(
    Duration::from_secs(2), /* interval */
    2.,                     /* exponential factor */
    3,                      /* max retry count */
)
.with_jitter(0.2 /* max_jitter_percentage */);

/// When there's an out-of-band request for a periodic poll, we want to retry quickly, because the UI is depending on the
/// request succeeding in a timely way. These are things like loading all object updates upon startup, checking the team
/// metadata when we visit the team page, etc.
pub const OUT_OF_BAND_REQUEST_RETRY_STRATEGY: RetryOption = RetryOption::exponential(
    Duration::from_millis(100), /* interval */
    5.,                         /* exponential factor */
    3,                          /* max retry count */
)
.with_jitter(0.5 /* max_jitter_percentage */);

// For listeners, retry up to 5 times, waiting between 10-40 seconds between retries.
pub const LISTENER_RETRY_STRATEGY: RetryOption = RetryOption::linear(
    Duration::from_secs(25), /* interval */
    5,                       /* max retry count */
)
.with_jitter(0.6 /* max_jitter_multiplier */);

/// Generalized retry function that gives callers control over retry classification and backoff.
///
/// The retry loop calls `attempt_fn` until:
/// * It succeeds
/// * It returns a non-retryable error, according to `retryable_fn`
/// * Attempts are exhausted due to `backoff_fn` returning `None`
///
/// `retryable_fn` classifies errors as retryable or not. It should return `true` if the failure
/// can be retried, `false` if it should abort the operation.
///
/// `backoff_fn` returns the backoff delay to use after N attempts.
pub(crate) async fn with_retry<T, E, F, S, B, R, Fut, SF>(
    operation: &str,
    mut attempt_fn: F,
    mut retryable_fn: R,
    mut sleep_fn: S,
    mut backoff_fn: B,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    E: Display,
    R: FnMut(&E) -> bool,
    B: FnMut(usize) -> Option<Duration>,
    S: FnMut(Duration) -> SF,
    Fut: Future<Output = Result<T, E>>,
    SF: Future<Output = ()>,
{
    let mut attempt = 1usize;
    loop {
        match attempt_fn().await {
            Ok(value) => break Ok(value),
            Err(err) if retryable_fn(&err) => {
                let delay = backoff_fn(attempt - 1);
                log::warn!(
                    "{operation}: transient error on attempt {attempt}, {}: {err:#}",
                    if delay.is_some() {
                        "retrying"
                    } else {
                        "retries exhausted"
                    }
                );

                match delay {
                    Some(delay) => {
                        sleep_fn(delay).await;
                        attempt += 1;
                    }
                    None => break Err(err),
                }
            }
            Err(err) => {
                log::warn!("{operation}: fatal error on attempt {attempt}: {err:#}");
                break Err(err);
            }
        }
    }
}

#[cfg(test)]
#[path = "retry_strategies_tests.rs"]
mod tests;
