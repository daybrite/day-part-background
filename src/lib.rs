// Copyright © The Daybrite Project
// SPDX-License-Identifier: MPL-2.0

//! Background work registered independently of the UI. Register tasks at link time so an
//! Android Worker or an iOS background launch can find them before any page is mounted.
//! Schedules are requests: the OS decides when work runs and may stop it before completion.
//! Use [`in_process`] for jobs that only need to run while this process stays alive.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

pub mod in_process;
mod native;
pub use day_core::linkme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Unsupported,
    InvalidRequest(String),
    Platform(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Retry,
    Failed,
}

/// The handler must check cancellation between bounded units of work and return promptly.
pub struct Context {
    pub payload: String,
    cancelled: Arc<AtomicBool>,
}
impl Context {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

pub struct TaskDefinition {
    pub identifier: &'static str,
    pub run: fn(Context) -> Outcome,
}

#[cfg(not(target_arch = "wasm32"))]
#[linkme::distributed_slice]
#[linkme(crate = day_core::linkme)]
pub static TASKS: [TaskDefinition];

/// Register a stable identifier and a function that does not require UI state.
#[macro_export]
macro_rules! register_task {
    ($id:literal, $handler:path) => {
        #[cfg(not(target_arch = "wasm32"))]
        const _: () = {
            #[$crate::linkme::distributed_slice($crate::TASKS)]
            #[linkme(crate = $crate::linkme)]
            static TASK: $crate::TaskDefinition = $crate::TaskDefinition {
                identifier: $id,
                run: $handler,
            };
        };
    };
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Schedule {
    pub identifier: String,
    pub payload: String,
    pub delay_secs: u64,
    /// Best-effort repeat interval, at least 15 minutes. Never an exact timer.
    pub interval_secs: Option<u64>,
    pub requires_network: bool,
    pub requires_charging: bool,
}
impl Schedule {
    pub fn new(identifier: impl Into<String>) -> Self {
        Self {
            identifier: identifier.into(),
            payload: String::new(),
            delay_secs: 0,
            interval_secs: None,
            requires_network: false,
            requires_charging: false,
        }
    }
    fn validate(&self) -> Result<(), Error> {
        if self.identifier.is_empty() || self.identifier.len() > 200 || self.payload.len() > 4096 {
            return Err(Error::InvalidRequest(
                "invalid identifier or payload larger than 4096 bytes".into(),
            ));
        }
        if self.delay_secs > 31_536_000
            || self
                .interval_secs
                .is_some_and(|n| !(900..=31_536_000).contains(&n))
        {
            return Err(Error::InvalidRequest(
                "repeat intervals must be 15 minutes to one year; delay cannot exceed one year"
                    .into(),
            ));
        }
        if !definitions()
            .iter()
            .any(|task| task.identifier == self.identifier)
        {
            return Err(Error::InvalidRequest(
                "task is not registered in this executable".into(),
            ));
        }
        Ok(())
    }
}

fn definitions() -> &'static [TaskDefinition] {
    #[cfg(not(target_arch = "wasm32"))]
    {
        &TASKS
    }
    #[cfg(target_arch = "wasm32")]
    {
        &[]
    }
}

/// Scheduler availability. Persistent scheduling may also require app manifest setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capabilities {
    /// Jobs can run while the app process is alive and the OS permits it to execute.
    pub in_process: bool,
    /// The OS can retain jobs and launch the app's services after its process exits.
    pub persistent: bool,
}

pub const fn capabilities() -> Capabilities {
    Capabilities {
        in_process: cfg!(any(
            target_os = "macos",
            target_os = "windows",
            all(target_os = "linux", not(target_env = "ohos")),
            target_os = "ios",
            target_os = "android",
        )),
        persistent: cfg!(any(target_os = "ios", target_os = "android")),
    }
}

/// Whether persistent OS scheduling is available. See [`capabilities`] for in-process work.
pub const fn supported() -> bool {
    capabilities().persistent
}

/// Persist an OS schedule; never falls back to in-process scheduling.
/// Replace a pending schedule for this identifier. An already executing handler receives
/// cancellation on Android; callers must make writes idempotent across retries and restarts.
pub async fn schedule(request: &Schedule) -> Result<(), Error> {
    request.validate()?;
    if !supported() {
        return Err(Error::Unsupported);
    }
    start()?;
    let text = serde_json::to_string(request).map_err(|e| Error::InvalidRequest(e.to_string()))?;
    native::schedule_native_future(&text)
        .await
        .map_err(bridge_error)
        .and_then(answer)
}
pub async fn cancel(identifier: &str) -> Result<(), Error> {
    if !supported() {
        return Err(Error::Unsupported);
    }
    native::cancel_native_future(identifier)
        .await
        .map_err(bridge_error)
        .and_then(answer)
}

/// Run a registered handler now, for foreground diagnostics. This does not exercise OS scheduling.
pub fn run_now(identifier: &str, payload: String) -> Result<Outcome, Error> {
    let task = definitions()
        .iter()
        .find(|task| task.identifier == identifier)
        .ok_or_else(|| Error::InvalidRequest("unknown task".into()))?;
    Ok((task.run)(Context {
        payload,
        cancelled: Arc::new(AtomicBool::new(false)),
    }))
}

type Runs = Mutex<HashMap<i64, Arc<AtomicBool>>>;
static RUNS: OnceLock<Runs> = OnceLock::new();
fn runs() -> &'static Runs {
    RUNS.get_or_init(Mutex::default)
}

#[derive(Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Delivery {
    Run {
        run: i64,
        identifier: String,
        payload: String,
    },
    Cancel {
        run: i64,
    },
}

fn start() -> Result<(), Error> {
    static STARTED: OnceLock<Result<(), Error>> = OnceLock::new();
    STARTED
        .get_or_init(|| {
            let mut ids = std::collections::HashSet::new();
            for definition in definitions() {
                if !ids.insert(definition.identifier) {
                    return Err(Error::InvalidRequest(
                        "duplicate background task identifier".into(),
                    ));
                }
            }
            native::events_native_stream(|item| {
                if let day_bridge::Item::Value(json) = item {
                    match serde_json::from_str::<Delivery>(&json) {
                        Ok(Delivery::Cancel { run }) => {
                            if let Some(cancelled) =
                                runs().lock().unwrap_or_else(|e| e.into_inner()).get(&run)
                            {
                                cancelled.store(true, Ordering::Release);
                            }
                        }
                        Ok(Delivery::Run {
                            run,
                            identifier,
                            payload,
                        }) => dispatch(run, identifier, payload),
                        Err(_) => {}
                    }
                }
            })
            .map_err(bridge_error)?;
            let mut identifiers: Vec<_> = ids.into_iter().collect();
            identifiers.sort_unstable();
            native::initialize(
                &serde_json::to_string(&identifiers)
                    .map_err(|e| Error::InvalidRequest(e.to_string()))?,
            )
            .map_err(bridge_error)
        })
        .clone()
}

fn dispatch(run: i64, identifier: String, payload: String) {
    let Some(task) = definitions()
        .iter()
        .find(|task| task.identifier == identifier)
    else {
        let _ = native::finish(run, "failed");
        return;
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut active = runs().lock().unwrap_or_else(|e| e.into_inner());
        if active.contains_key(&run) {
            return;
        }
        active.insert(run, cancelled.clone());
    }
    let handler = task.run;
    #[cfg(not(target_arch = "wasm32"))]
    {
        let spawned = std::thread::Builder::new()
            .name("day-background".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| handler(Context { payload, cancelled }))
                    .unwrap_or(Outcome::Failed);
                runs()
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&run);
                let value = match result {
                    Outcome::Success => "success",
                    Outcome::Retry => "retry",
                    Outcome::Failed => "failed",
                };
                let _ = native::finish(run, value);
            });
        if spawned.is_err() {
            runs()
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&run);
            let _ = native::finish(run, "failed");
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (handler, payload, cancelled);
    }
}

#[cfg(any(target_os = "ios", target_os = "android"))]
#[day_core::linkme::distributed_slice(day_core::services::SERVICES)]
#[linkme(crate = day_core::linkme)]
static SERVICE: day_core::services::Service = day_core::services::Service {
    name: "background",
    initialize: || {
        let _ = start();
    },
};

fn bridge_error(e: day_bridge::Error) -> Error {
    if matches!(e, day_bridge::Error::Unsupported) {
        Error::Unsupported
    } else {
        Error::Platform(e.to_string())
    }
}
fn answer(value: String) -> Result<(), Error> {
    if value.is_empty() {
        Ok(())
    } else {
        Err(Error::Platform(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    register_task!("dev.daybrite.fixture", fixture);
    fn fixture(context: Context) -> Outcome {
        if context.is_cancelled() {
            Outcome::Retry
        } else {
            Outcome::Success
        }
    }
    #[test]
    fn schedules_require_registered_handlers_and_bounded_payloads() {
        assert!(Schedule::new("unknown").validate().is_err());
        let mut request = Schedule::new("dev.daybrite.fixture");
        assert!(request.validate().is_ok());
        request.interval_secs = Some(30);
        assert!(request.validate().is_err());
        request.interval_secs = Some(900);
        request.payload = "x".repeat(4097);
        assert!(request.validate().is_err());
    }
    #[test]
    fn cancellation_reaches_a_running_handler() {
        let cancelled = Arc::new(AtomicBool::new(true));
        assert_eq!(
            fixture(Context {
                payload: String::new(),
                cancelled
            }),
            Outcome::Retry
        );
        assert_eq!(
            run_now("dev.daybrite.fixture", String::new()),
            Ok(Outcome::Success)
        );
    }

    #[test]
    fn in_process_constraints_are_rejected_instead_of_ignored() {
        let mut request = Schedule::new("dev.daybrite.fixture");
        request.requires_network = true;
        assert_eq!(in_process::schedule(&request), Err(Error::Unsupported));
        request.requires_network = false;
        request.requires_charging = true;
        assert_eq!(in_process::schedule(&request), Err(Error::Unsupported));
    }
}
