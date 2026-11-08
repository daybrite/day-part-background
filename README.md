<!--
Copyright © The Daybrite Project
SPDX-License-Identifier: MPL-2.0
-->

# day-part-background

Schedule short tasks such as retrying a sync or refreshing cached data. Register handlers that
can run independently of a page, then choose how long the schedule needs to survive.

| Scheduler | Lifetime | Platforms |
| --- | --- | --- |
| Persistent | The OS retains requests after the app process exits. | iOS and Android |
| In-process | Pending jobs disappear when the process exits. | iOS, Android, macOS, Windows, and Linux |

Check `capabilities()` before choosing. Persistent scheduling never falls back to in-process
work. Web and HarmonyOS support neither scheduler.

## Add to your app

```sh
cargo add 'day-part-background@0.*'
```

Build with the Day CLI. Android uses WorkManager; iOS uses BGTaskScheduler and requires
[declared task identifiers and background modes](docs/background.md#platform-setup).

## Register and schedule a task

```rust
use day_part_background::{Context, Outcome, Schedule};

day_part_background::register_task!("dev.example.refresh", refresh);

fn refresh(context: Context) -> Outcome {
    if context.is_cancelled() { return Outcome::Retry; }
    // Process a bounded chunk of persisted work; check cancellation between chunks.
    Outcome::Success
}

async fn schedule_refresh() -> Result<(), day_part_background::Error> {
    let mut request = Schedule::new("dev.example.refresh");
    request.requires_network = true;
    request.interval_secs = Some(15 * 60);
    day_part_background::schedule(&request).await
}
```

Handlers run on worker threads and must tolerate retries. Persist inputs before scheduling;
a handler cannot depend on an open window or page signals. The OS chooses the execution time
and may cancel or defer work. Keep a foreground path for actions users need immediately.

For work that only needs to run while the process stays alive, use `in_process::schedule` with
the same registered handler. It supports delays and retries but rejects network and charging
constraints. Cancellation is cooperative in both schedulers.

## Documentation and examples

- [Scheduling rules, cancellation, and platform setup](docs/background.md)
- [Run Background Lab](demo/README.md)
- [CI, crate reservation, and trusted publishing](docs/releases.md)

```sh
cargo test
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps --open
```

[MPL-2.0 license](LICENSE)
