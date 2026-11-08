use day::prelude::*;
use day_part_background as work;
use std::sync::Mutex;

day::resources!();
day::day_start!(options: res::window_options(), root);

const TASK: &str = "dev.daybrite.background.demo.refresh";
const COUNT: &str = "background-demo.completed";
static JOURNAL: Mutex<()> = Mutex::new(());
work::register_task!("dev.daybrite.background.demo.refresh", refresh);

fn refresh(context: work::Context) -> work::Outcome {
    let _lock = JOURNAL.lock().unwrap_or_else(|e| e.into_inner());
    if context.is_cancelled() {
        return work::Outcome::Retry;
    }
    let count = completed().saturating_add(1);
    if day::prefs::set(COUNT, &count.to_string()) {
        work::Outcome::Success
    } else {
        work::Outcome::Failed
    }
}
fn completed() -> u64 {
    day::prefs::get(COUNT)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

pub fn root() -> impl Piece {
    let status = Signal::new(res::str::ready().format());
    let count = Signal::new(completed());
    scroll(
        column((
            label(res::str::app_title())
                .font(Font::Title)
                .id(day::id!(title)),
            label(res::str::intro()),
            label(if work::supported() {
                res::str::supported()
            } else {
                res::str::unsupported()
            }),
            button(res::str::schedule())
                .id(day::id!(schedule))
                .enabled(work::capabilities().persistent)
                .action(move || {
                    day::task(async move {
                        let mut request = work::Schedule::new(TASK);
                        request.delay_secs = 30;
                        status.set(match work::schedule(&request).await {
                            Ok(()) => res::str::scheduled().format(),
                            Err(error) => res::str::operation_error(error.to_string()).format(),
                        });
                    });
                }),
            button(res::str::repeat())
                .id(day::id!(repeat))
                .enabled(work::capabilities().persistent)
                .action(move || {
                    day::task(async move {
                        let mut request = work::Schedule::new(TASK);
                        request.interval_secs = Some(900);
                        status.set(match work::schedule(&request).await {
                            Ok(()) => res::str::scheduled().format(),
                            Err(error) => res::str::operation_error(error.to_string()).format(),
                        });
                    });
                }),
            button(res::str::cancel())
                .id(day::id!(cancel))
                .enabled(work::capabilities().persistent)
                .action(move || {
                    day::task(async move {
                        status.set(match work::cancel(TASK).await {
                            Ok(()) => res::str::cancelled().format(),
                            Err(error) => res::str::operation_error(error.to_string()).format(),
                        });
                    });
                }),
            label(res::str::in_process_help()),
            button(res::str::in_process_schedule())
                .id(day::id!(in_process_schedule))
                .enabled(work::capabilities().in_process)
                .action(move || {
                    let mut request = work::Schedule::new(TASK);
                    request.delay_secs = 5;
                    status.set(match work::in_process::schedule(&request) {
                        Ok(()) => res::str::in_process_scheduled().format(),
                        Err(error) => res::str::operation_error(error.to_string()).format(),
                    });
                }),
            button(res::str::in_process_cancel())
                .id(day::id!(in_process_cancel))
                .enabled(work::capabilities().in_process)
                .action(move || {
                    status.set(match work::in_process::cancel(TASK) {
                        Ok(()) => res::str::cancelled().format(),
                        Err(error) => res::str::operation_error(error.to_string()).format(),
                    });
                }),
            label(res::str::diagnostic_help()),
            button(res::str::run_now())
                .id(day::id!(run_now))
                .enabled(cfg!(not(target_arch = "wasm32")))
                .action(move || {
                    status.set(match work::run_now(TASK, String::new()) {
                        Ok(work::Outcome::Success) => res::str::completed().format(),
                        Ok(_) => res::str::failed().format(),
                        Err(error) => res::str::operation_error(error.to_string()).format(),
                    });
                    count.set(completed());
                }),
            button(res::str::refresh())
                .id(day::id!(refresh))
                .action(move || count.set(completed())),
            label(move || res::str::run_count(count.get().to_string())).id(day::id!(count)),
            label(status).id(day::id!(status)),
        ))
        .spacing(Space::M)
        .padding(Space::L),
    )
}
