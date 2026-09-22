//! Staggered, bounded connection attempts. Dropping the race aborts losers.
use std::future::Future;
use std::time::Duration;
use tokio::task::JoinSet;
use tokio::time::{sleep_until, Instant};

pub async fn first_ready<T, R, F, Fut>(
    candidates: Vec<T>,
    stagger: Duration,
    limit: usize,
    dial: F,
    mut failed: impl FnMut(T),
) -> Option<R>
where
    T: Clone + Send + 'static,
    R: Send + 'static,
    F: Fn(T) -> Fut,
    Fut: Future<Output = Option<R>> + Send + 'static,
{
    let mut candidates = candidates.into_iter().peekable();
    let mut pending: JoinSet<(T, Option<R>)> = JoinSet::new();
    let mut next_launch = Instant::now();
    loop {
        if pending.is_empty() && candidates.peek().is_none() {
            return None;
        }
        tokio::select! {
            biased;
            result = pending.join_next(), if !pending.is_empty() => {
                if let Some(Ok((candidate, result))) = result {
                    if result.is_some() {
                        // JoinSet's drop cancels every unfinished attempt and
                        // drops any connected socket that lost the race.
                        return result;
                    }
                    failed(candidate);
                }
                // Replace a failed attempt immediately, without an extra delay.
                next_launch = Instant::now();
            }
            _ = sleep_until(next_launch), if pending.len() < limit.max(1) && candidates.peek().is_some() => {
                let candidate = candidates.next().expect("checked above");
                let future = dial(candidate.clone());
                pending.spawn(async move { (candidate, future.await) });
                next_launch = Instant::now() + stagger;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    #[tokio::test(start_paused = true)]
    async fn fast_backup_does_not_wait_for_first_timeout() {
        let start = Instant::now();
        let result = first_ready(
            vec![0, 1],
            Duration::from_millis(100),
            2,
            |n| async move {
                tokio::time::sleep(Duration::from_millis(if n == 0 { 1200 } else { 20 })).await;
                Some(n)
            },
            |_| panic!("cancelled loser is not a failure"),
        )
        .await;
        assert_eq!(result, Some(1));
        assert_eq!(start.elapsed(), Duration::from_millis(120));
    }

    #[tokio::test(start_paused = true)]
    async fn preferred_success_does_not_launch_backups() {
        let launches = AtomicUsize::new(0);
        let result = first_ready(
            vec![0, 1, 2],
            Duration::from_millis(100),
            3,
            |n| {
                launches.fetch_add(1, Ordering::SeqCst);
                async move { Some(n) }
            },
            |_| {},
        )
        .await;
        assert_eq!(result, Some(0));
        assert_eq!(launches.load(Ordering::SeqCst), 1);
    }

    struct Active(Arc<AtomicUsize>);
    impl Drop for Active {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_cleans_up_and_concurrency_is_bounded() {
        let active = Arc::new(AtomicUsize::new(0));
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            first_ready(
                (0..20).collect(),
                Duration::from_millis(100),
                3,
                |_: usize| {
                    let active = active.clone();
                    async move {
                        assert!(active.fetch_add(1, Ordering::SeqCst) < 3);
                        let _active = Active(active);
                        std::future::pending::<Option<()>>().await
                    }
                },
                |_| panic!("cancellation must not penalize a relay"),
            ),
        )
        .await;
        assert!(result.is_err());
        tokio::task::yield_now().await;
        assert_eq!(active.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn all_failures_are_reported_and_empty_input_finishes() {
        let mut failures = Vec::new();
        let result = first_ready(
            vec![0, 1, 2],
            Duration::from_secs(1),
            2,
            |_| async { None::<()> },
            |n| failures.push(n),
        )
        .await;
        assert_eq!(result, None);
        assert_eq!(failures, vec![0, 1, 2]);
        assert_eq!(
            first_ready(
                Vec::<usize>::new(),
                Duration::ZERO,
                2,
                |_| async { Some(()) },
                |_| {}
            )
            .await,
            None
        );
    }
}
