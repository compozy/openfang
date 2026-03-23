//! Serialized access control for Codex model requests.

use std::{future::Future, sync::Arc, time::Duration};

use arky_provider::ProviderError;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore},
    time::timeout,
};

/// Guard representing one scheduled model access slot.
#[derive(Debug)]
pub struct SchedulerPermit {
    _permit: OwnedSemaphorePermit,
    _label: String,
}

/// Serializes access to the Codex app-server model lane.
#[derive(Debug, Clone)]
pub struct Scheduler {
    semaphore: Arc<Semaphore>,
    acquire_timeout: Duration,
    max_queued_requests: usize,
    queued_requests: Arc<Mutex<usize>>,
}

impl Scheduler {
    /// Creates a scheduler that allows only one active model request.
    #[must_use]
    pub fn new(acquire_timeout: Duration) -> Self {
        Self::with_limits(1, acquire_timeout, usize::MAX)
    }

    /// Creates a scheduler with explicit concurrency and queue limits.
    #[must_use]
    pub fn with_limits(
        limit: usize,
        acquire_timeout: Duration,
        max_queued_requests: usize,
    ) -> Self {
        Self {
            semaphore: Arc::new(Semaphore::new(limit.max(1))),
            acquire_timeout,
            max_queued_requests: max_queued_requests.max(1),
            queued_requests: Arc::new(Mutex::new(0)),
        }
    }

    /// Acquires a scheduling permit, waiting up to the configured timeout.
    pub async fn acquire(
        &self,
        label: impl Into<String>,
    ) -> Result<SchedulerPermit, ProviderError> {
        let label = label.into();
        if let Ok(permit) = self.semaphore.clone().try_acquire_owned() {
            return Ok(SchedulerPermit {
                _permit: permit,
                _label: label,
            });
        }

        self.reserve_queue_slot(&label).await?;

        let permit_result =
            timeout(self.acquire_timeout, self.semaphore.clone().acquire_owned()).await;
        self.release_queue_slot().await;

        let permit = permit_result
            .map_err(|_| {
                ProviderError::stream_interrupted(format!(
                    "timed out waiting for scheduler slot for `{label}`"
                ))
            })?
            .map_err(|_| {
                ProviderError::stream_interrupted(format!(
                    "scheduler closed while waiting for `{label}`"
                ))
            })?;

        Ok(SchedulerPermit {
            _permit: permit,
            _label: label,
        })
    }

    /// Runs a future while holding one scheduler slot.
    pub async fn run<F, T>(&self, label: impl Into<String>, future: F) -> Result<T, ProviderError>
    where
        F: Future<Output = Result<T, ProviderError>>,
    {
        let _permit = self.acquire(label).await?;
        future.await
    }

    async fn reserve_queue_slot(&self, label: &str) -> Result<(), ProviderError> {
        let mut queued_requests = self.queued_requests.lock().await;
        if *queued_requests >= self.max_queued_requests {
            return Err(ProviderError::stream_interrupted(format!(
                "request queue overflow while scheduling `{label}`"
            )));
        }

        *queued_requests += 1;
        drop(queued_requests);
        Ok(())
    }

    async fn release_queue_slot(&self) {
        let mut queued_requests = self.queued_requests.lock().await;
        *queued_requests = queued_requests.saturating_sub(1);
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new(Duration::from_secs(300))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arky_error::ClassifiedError;
    use pretty_assertions::assert_eq;
    use tokio::{
        sync::{Barrier, oneshot},
        time::{Duration, sleep},
    };

    use super::Scheduler;

    #[tokio::test]
    async fn scheduler_should_serialize_access_until_first_permit_releases() {
        let scheduler = Scheduler::new(Duration::from_secs(1));
        let barrier = Arc::new(Barrier::new(2));
        let (first_release_tx, first_release_rx) = oneshot::channel::<()>();
        let (second_ran_tx, mut second_ran_rx) = oneshot::channel::<()>();

        let first_scheduler = scheduler.clone();
        let first_barrier = barrier.clone();
        let first_task = tokio::spawn(async move {
            let _permit = first_scheduler
                .acquire("first")
                .await
                .expect("first permit should acquire");
            first_barrier.wait().await;
            let _ = first_release_rx.await;
        });

        let second_scheduler = scheduler.clone();
        let second_barrier = barrier.clone();
        let second_task = tokio::spawn(async move {
            second_barrier.wait().await;
            let _permit = second_scheduler
                .acquire("second")
                .await
                .expect("second permit should eventually acquire");
            let _ = second_ran_tx.send(());
        });

        sleep(Duration::from_millis(30)).await;
        assert_eq!(second_ran_rx.try_recv().is_err(), true);

        let _ = first_release_tx.send(());
        first_task.await.expect("first task should finish");
        second_task.await.expect("second task should finish");
        second_ran_rx.await.expect("second task should run");
    }

    #[tokio::test]
    async fn scheduler_should_reject_queue_overflow() {
        let scheduler = Scheduler::with_limits(1, Duration::from_secs(1), 1);
        let first_permit = scheduler
            .acquire("active")
            .await
            .expect("first permit should acquire");

        let blocked_scheduler = scheduler.clone();
        let blocked = tokio::spawn(async move {
            blocked_scheduler
                .acquire("queued")
                .await
                .expect("second request should occupy the queue");
        });

        sleep(Duration::from_millis(30)).await;

        let error = scheduler
            .acquire("overflow")
            .await
            .expect_err("overflowing request should fail");

        assert_eq!(error.error_code(), "PROVIDER_STREAM_INTERRUPTED");
        assert_eq!(error.to_string().contains("request queue overflow"), true);

        drop(first_permit);
        blocked.abort();
        let _ = blocked.await;
    }
}
