//! Spawn-failure cooldown tracking for the Claude Code CLI.

use std::sync::Arc;

use tokio::{
    sync::Mutex,
    time::{Duration, Instant, sleep_until},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpawnFailureState {
    consecutive_failures: u32,
    cooldown_until: Option<Instant>,
}

/// Circuit-breaker policy applied after repeated spawn failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnFailurePolicy {
    /// Number of consecutive failures allowed before backoff begins.
    pub max_consecutive_failures: u32,
    /// Cooldown duration applied once the threshold is reached.
    pub cooldown: Duration,
}

impl Default for SpawnFailurePolicy {
    fn default() -> Self {
        Self {
            max_consecutive_failures: 3,
            cooldown: Duration::from_secs(10),
        }
    }
}

/// Current attempt status for a tracked binary spawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnAttemptStatus {
    /// Whether a new spawn attempt can proceed immediately.
    pub can_attempt: bool,
    /// Current consecutive failure count after state normalization.
    pub consecutive_failures: u32,
    /// Delay that must elapse before another attempt is allowed.
    pub retry_after: Option<Duration>,
}

/// Result returned when a new spawn failure is recorded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnFailureRecord {
    /// Whether the failure triggered a cooldown.
    pub should_backoff: bool,
    /// Current consecutive failure count.
    pub consecutive_failures: u32,
    /// Duration until the next attempt is allowed.
    pub retry_after: Option<Duration>,
}

/// Shared tracker used by the provider to avoid rapid spawn loops.
#[derive(Debug, Clone)]
pub struct SpawnFailureTracker {
    policy: SpawnFailurePolicy,
    state: Arc<Mutex<SpawnFailureState>>,
}

impl SpawnFailureTracker {
    /// Creates a tracker with the supplied policy.
    #[must_use]
    pub fn new(policy: SpawnFailurePolicy) -> Self {
        Self {
            policy,
            state: Arc::new(Mutex::new(SpawnFailureState {
                consecutive_failures: 0,
                cooldown_until: None,
            })),
        }
    }

    fn normalize(state: &mut SpawnFailureState) {
        if let Some(until) = state.cooldown_until
            && Instant::now() >= until
        {
            state.consecutive_failures = 0;
            state.cooldown_until = None;
        }
    }

    /// Reports whether a spawn can be attempted immediately.
    pub async fn status(&self) -> SpawnAttemptStatus {
        let mut state = self.state.lock().await;
        Self::normalize(&mut state);

        if let Some(until) = state.cooldown_until {
            return SpawnAttemptStatus {
                can_attempt: false,
                consecutive_failures: state.consecutive_failures,
                retry_after: Some(until.saturating_duration_since(Instant::now())),
            };
        }

        SpawnAttemptStatus {
            can_attempt: true,
            consecutive_failures: state.consecutive_failures,
            retry_after: None,
        }
    }

    /// Waits until the cooldown expires, if one is active.
    pub async fn wait_until_ready(&self) {
        let until = {
            let mut state = self.state.lock().await;
            Self::normalize(&mut state);
            state.cooldown_until
        };

        if let Some(until) = until {
            sleep_until(until).await;
            let mut state = self.state.lock().await;
            Self::normalize(&mut state);
        }
    }

    /// Records a failed spawn attempt.
    pub async fn record_failure(&self) -> SpawnFailureRecord {
        let mut state = self.state.lock().await;
        Self::normalize(&mut state);
        state.consecutive_failures = state.consecutive_failures.saturating_add(1);

        if state.consecutive_failures >= self.policy.max_consecutive_failures {
            state.cooldown_until = Some(Instant::now() + self.policy.cooldown);
            return SpawnFailureRecord {
                should_backoff: true,
                consecutive_failures: state.consecutive_failures,
                retry_after: Some(self.policy.cooldown),
            };
        }

        SpawnFailureRecord {
            should_backoff: false,
            consecutive_failures: state.consecutive_failures,
            retry_after: None,
        }
    }

    /// Records a successful spawn and clears the breaker state.
    pub async fn record_success(&self) {
        let mut state = self.state.lock().await;
        state.consecutive_failures = 0;
        state.cooldown_until = None;
    }
}

impl Default for SpawnFailureTracker {
    fn default() -> Self {
        Self::new(SpawnFailurePolicy::default())
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;

    use super::{SpawnFailurePolicy, SpawnFailureTracker};

    #[tokio::test(start_paused = true)]
    async fn cooldown_should_delay_subsequent_attempts() {
        let tracker = SpawnFailureTracker::new(SpawnFailurePolicy {
            max_consecutive_failures: 1,
            cooldown: tokio::time::Duration::from_secs(5),
        });

        let first_failure = tracker.record_failure().await;
        assert_eq!(first_failure.should_backoff, true);

        let status = tracker.status().await;
        assert_eq!(status.can_attempt, false);
        assert_eq!(status.consecutive_failures, 1);
        assert_eq!(
            status.retry_after,
            Some(tokio::time::Duration::from_secs(5))
        );

        let waiting = tokio::spawn({
            let tracker = tracker.clone();
            async move {
                tracker.wait_until_ready().await;
            }
        });

        tokio::task::yield_now().await;
        assert_eq!(waiting.is_finished(), false);

        tokio::time::advance(tokio::time::Duration::from_secs(5)).await;
        waiting.await.expect("cooldown task should finish");

        let ready = tracker.status().await;
        assert_eq!(ready.can_attempt, true);
        assert_eq!(ready.consecutive_failures, 0);
    }

    #[tokio::test]
    async fn success_should_reset_failure_state() {
        let tracker = SpawnFailureTracker::new(SpawnFailurePolicy {
            max_consecutive_failures: 2,
            cooldown: tokio::time::Duration::from_secs(1),
        });

        let first = tracker.record_failure().await;
        assert_eq!(first.should_backoff, false);

        tracker.record_success().await;

        let status = tracker.status().await;
        assert_eq!(status.can_attempt, true);
        assert_eq!(status.consecutive_failures, 0);
    }
}
