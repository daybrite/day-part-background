// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

//! Jobs held in memory, with no startup registration or OS keep-alive request.
//!
//! Jobs disappear when the process exits and pause if the OS suspends the process.
//! Each identifier has one schedule, separate from the persistent scheduler's identifiers.
//! Cancellation is cooperative. A replacement may start before the old handler returns.
//! Delays and repeats are minimums measured with a monotonic clock; repeats wait after
//! completion. `Retry` waits 15 minutes, `Failed` ends the schedule (including repeats).

use crate::{Error, Schedule};

/// Replace an in-process schedule. Network/charging constraints are unsupported and rejected.
/// Up to 64 worker threads may be active, including cancelled handlers that have not returned.
pub fn schedule(request: &Schedule) -> Result<(), Error> {
    if !crate::capabilities().in_process {
        return Err(Error::Unsupported);
    }
    request.validate()?;
    if request.requires_network || request.requires_charging {
        return Err(Error::Unsupported);
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        runtime::scheduler().schedule(request)
    }
    #[cfg(target_arch = "wasm32")]
    Err(Error::Unsupported)
}

/// Remove an in-process schedule and signal cancellation to its active handler.
/// Does not affect persistent OS work with the same identifier. Missing schedules are a no-op.
pub fn cancel(identifier: &str) -> Result<(), Error> {
    #[cfg(target_arch = "wasm32")]
    let _ = identifier;
    if !crate::capabilities().in_process {
        return Err(Error::Unsupported);
    }
    #[cfg(not(target_arch = "wasm32"))]
    runtime::scheduler().cancel(identifier);
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
mod runtime {
    use super::*;
    use crate::{Context, Outcome};
    use std::collections::HashMap;
    use std::sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    };
    use std::time::Duration;

    const MAX_WORKERS: usize = 64;
    const RETRY_DELAY: Duration = Duration::from_secs(900);

    #[derive(Default)]
    struct Job {
        cancelled: Arc<AtomicBool>,
        gate: Mutex<()>,
        wake: Condvar,
    }
    impl Job {
        fn stop(&self) {
            let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
            self.cancelled.store(true, Ordering::Release);
            self.wake.notify_all();
        }

        /// False means cancelled; true means the minimum delay elapsed.
        fn wait(&self, delay: Duration) -> bool {
            let guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
            let _result = self
                .wake
                .wait_timeout_while(guard, delay, |_| !self.cancelled.load(Ordering::Acquire))
                .unwrap_or_else(|e| e.into_inner());
            !self.cancelled.load(Ordering::Acquire)
        }
    }

    #[derive(Default)]
    struct State {
        jobs: HashMap<String, Arc<Job>>,
        workers: usize,
    }
    #[derive(Default)]
    pub(super) struct Scheduler {
        state: Mutex<State>,
    }
    pub(super) fn scheduler() -> &'static Arc<Scheduler> {
        static SCHEDULER: OnceLock<Arc<Scheduler>> = OnceLock::new();
        SCHEDULER.get_or_init(|| Arc::new(Scheduler::default()))
    }

    impl Scheduler {
        pub(super) fn schedule(self: &Arc<Self>, request: &Schedule) -> Result<(), Error> {
            let mut matching = crate::definitions()
                .iter()
                .filter(|task| task.identifier == request.identifier);
            let handler = matching
                .next()
                .ok_or_else(|| Error::InvalidRequest("unknown task".into()))?
                .run;
            if matching.next().is_some() {
                return Err(Error::InvalidRequest(
                    "duplicate background task identifier".into(),
                ));
            }
            self.spawn(request.clone(), handler)
        }

        fn spawn(
            self: &Arc<Self>,
            request: Schedule,
            handler: impl Fn(Context) -> Outcome + Send + 'static,
        ) -> Result<(), Error> {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.workers >= MAX_WORKERS {
                return Err(Error::Platform("in-process worker limit reached".into()));
            }
            let scheduler = self.clone();
            let job = Arc::new(Job::default());
            let worker_job = job.clone();
            let identifier = request.identifier.clone();
            std::thread::Builder::new()
                .name("day-background-local".into())
                .spawn(move || {
                    // Scheduling installs this job and cancels its predecessor before execution.
                    drop(scheduler.state.lock().unwrap_or_else(|e| e.into_inner()));
                    let mut delay = Duration::from_secs(request.delay_secs);
                    while worker_job.wait(delay) {
                        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            handler(Context {
                                payload: request.payload.clone(),
                                cancelled: worker_job.cancelled.clone(),
                            })
                        }))
                        .unwrap_or(Outcome::Failed);
                        match next_delay(result, request.interval_secs) {
                            Some(next) => delay = next,
                            None => break,
                        }
                    }
                    let mut state = scheduler.state.lock().unwrap_or_else(|e| e.into_inner());
                    state.workers -= 1;
                    if state
                        .jobs
                        .get(&request.identifier)
                        .is_some_and(|current| Arc::ptr_eq(current, &worker_job))
                    {
                        state.jobs.remove(&request.identifier);
                    }
                })
                .map_err(|e| Error::Platform(e.to_string()))?;
            state.workers += 1;
            if let Some(previous) = state.jobs.insert(identifier, job) {
                previous.stop();
            }
            Ok(())
        }

        pub(super) fn cancel(&self, identifier: &str) {
            if let Some(job) = self
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .jobs
                .remove(identifier)
            {
                job.stop();
            }
        }
    }

    fn next_delay(result: Outcome, interval: Option<u64>) -> Option<Duration> {
        match result {
            Outcome::Success => interval.map(Duration::from_secs),
            Outcome::Retry => Some(RETRY_DELAY),
            Outcome::Failed => None,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::mpsc;
        const TIMEOUT: Duration = Duration::from_secs(3);

        #[test]
        fn panic_ends_a_repeating_job_and_releases_its_worker() {
            let scheduler = Arc::new(Scheduler::default());
            let (tx, rx) = mpsc::channel::<()>();
            let mut request = Schedule::new("panic-fixture");
            request.interval_secs = Some(900);
            scheduler
                .spawn(request, move |_| {
                    let _keep_until_handler_drops = &tx;
                    panic!("synthetic handler failure");
                })
                .unwrap();
            assert_eq!(
                rx.recv_timeout(TIMEOUT),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
            let state = scheduler.state.lock().unwrap();
            assert_eq!(state.workers, 0);
            assert!(state.jobs.is_empty());
        }

        #[test]
        fn cancellation_wakes_pending_job_without_running_handler() {
            let scheduler = Arc::new(Scheduler::default());
            let mut request = Schedule::new("pending-fixture");
            request.delay_secs = 3600;
            let (tx, rx) = mpsc::channel();
            scheduler
                .spawn(request, move |_| {
                    tx.send(()).unwrap();
                    Outcome::Success
                })
                .unwrap();
            scheduler.cancel("pending-fixture");
            // Sender is dropped when the worker exits, rather than waiting out its hour delay.
            assert_eq!(
                rx.recv_timeout(TIMEOUT),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        }

        #[test]
        fn replacing_running_job_cancels_it_and_preserves_replacement() {
            let scheduler = Arc::new(Scheduler::default());
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (stopped_tx, stopped_rx) = mpsc::channel();
            scheduler
                .spawn(Schedule::new("replace-fixture"), move |context| {
                    entered_tx.send(()).unwrap();
                    release_rx.recv_timeout(TIMEOUT).unwrap();
                    stopped_tx.send(context.is_cancelled()).unwrap();
                    Outcome::Success
                })
                .unwrap();
            entered_rx.recv_timeout(TIMEOUT).unwrap();
            let mut replacement = Schedule::new("replace-fixture");
            replacement.delay_secs = 3600;
            let (replacement_tx, replacement_rx) = mpsc::channel();
            scheduler
                .spawn(replacement, move |_| {
                    replacement_tx.send(()).unwrap();
                    Outcome::Success
                })
                .unwrap();
            release_tx.send(()).unwrap();
            assert!(stopped_rx.recv_timeout(TIMEOUT).unwrap());
            // The old thread must exit before checking that its cleanup kept the new job.
            assert_eq!(
                entered_rx.recv_timeout(TIMEOUT),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
            assert!(
                scheduler
                    .state
                    .lock()
                    .unwrap()
                    .jobs
                    .contains_key("replace-fixture")
            );
            scheduler.cancel("replace-fixture");
            assert_eq!(
                replacement_rx.recv_timeout(TIMEOUT),
                Err(mpsc::RecvTimeoutError::Disconnected)
            );
        }

        #[test]
        fn worker_limit_counts_cancelled_handlers_until_they_return() {
            let scheduler = Arc::new(Scheduler::default());
            // Simulate occupied workers; no need to allocate 64 real threads.
            scheduler.state.lock().unwrap().workers = MAX_WORKERS;
            let result = scheduler.spawn(Schedule::new("limit-fixture"), |_| Outcome::Success);
            assert!(matches!(result, Err(Error::Platform(_))));
            assert!(scheduler.state.lock().unwrap().jobs.is_empty());
        }

        #[test]
        fn success_retry_and_failure_have_distinct_repeat_behavior() {
            assert_eq!(next_delay(Outcome::Success, None), None);
            assert_eq!(
                next_delay(Outcome::Success, Some(1800)),
                Some(Duration::from_secs(1800))
            );
            assert_eq!(next_delay(Outcome::Retry, None), Some(RETRY_DELAY));
            assert_eq!(next_delay(Outcome::Failed, Some(1800)), None);
        }
    }
}
