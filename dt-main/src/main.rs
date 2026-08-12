use std::env;

use dt_precheck::{config::task_config::PrecheckTaskConfig, do_precheck, load_precheck_configs};
use dt_task::task_runner::TaskRunner;
use tokio_util::sync::CancellationToken;

mod shutdown;

use shutdown::Shutdown;

/// Config/init problems: nothing ran.
const EXIT_INIT_FAILED: i32 = 2;
/// The task itself failed.
const EXIT_TASK_FAILED: i32 = 3;

#[tokio::main]
async fn main() {
    env::set_var("RUST_BACKTRACE", "1");
    let code = run().await;
    std::process::exit(code);
}

async fn run() -> i32 {
    // Installed before anything long-running starts, so a signal during connection setup is
    // still handled by us rather than terminating the process by default.
    let cancel_token = CancellationToken::new();
    let mut signals = shutdown::spawn_signal_listener(cancel_token.clone());

    let task_config = match env::args().nth(1) {
        Some(task_config) => task_config,
        None => {
            eprintln!("no task_config provided in args");
            return EXIT_INIT_FAILED;
        }
    };

    let is_precheck = match PrecheckTaskConfig::is_precheck(&task_config) {
        Ok(is_precheck) => is_precheck,
        Err(e) => {
            eprintln!("task config init failed: {e:#}");
            return EXIT_INIT_FAILED;
        }
    };

    if is_precheck {
        let (precheck_config, task_config) = match load_precheck_configs(&task_config) {
            Ok(configs) => configs,
            Err(e) => {
                eprintln!("precheck init failed: {e:#}");
                return EXIT_INIT_FAILED;
            }
        };
        // A precheck writes no positions and has nothing to drain, so there is nothing to wait
        // for: a signal ends it at once.
        return match shutdown::supervise(
            do_precheck(precheck_config, task_config),
            &mut signals,
            std::time::Duration::ZERO,
        )
        .await
        {
            Shutdown::Completed(Ok(())) => 0,
            Shutdown::Completed(Err(e)) => {
                eprintln!("precheck failed: {e:#}");
                EXIT_TASK_FAILED
            }
            Shutdown::Interrupted { signal, .. } | Shutdown::TimedOut { signal } => {
                eprintln!("precheck interrupted by signal {signal}");
                shutdown::exit_code_for_signal(signal)
            }
        };
    }

    let runner = match TaskRunner::new(&task_config) {
        Ok(runner) => runner,
        Err(e) => {
            eprintln!("task runner init failed: {e:#}");
            return EXIT_INIT_FAILED;
        }
    };

    let grace = shutdown::shutdown_timeout_from_env();
    match shutdown::supervise(
        runner.start_task_with_cancel(cancel_token),
        &mut signals,
        grace,
    )
    .await
    {
        Shutdown::Completed(Ok(())) => 0,
        Shutdown::Completed(Err(e)) => {
            eprintln!("task execution failed: {e:#}");
            EXIT_TASK_FAILED
        }
        // The run stopped short of its end, so it must not look like a success — even when the
        // shutdown itself went perfectly. And 128+signal claims a *clean* stop, so a drain that
        // failed on the way out reports the failure instead: its position may be missing.
        Shutdown::Interrupted { signal, result } => match result {
            Ok(()) => {
                eprintln!("task stopped gracefully after signal {signal}: position recorded");
                shutdown::exit_code_for_signal(signal)
            }
            // A wait released by the cancellation itself is the shutdown working, not a failure
            // (ADR 0002 demotes those to `Error::Cancelled`); anything else really did fail.
            Err(e) if is_cancellation(&e) => {
                eprintln!("task stopped gracefully after signal {signal}: position recorded ({e:#})");
                shutdown::exit_code_for_signal(signal)
            }
            Err(e) => {
                eprintln!("task failed while shutting down on signal {signal}: {e:#}");
                EXIT_TASK_FAILED
            }
        },
        Shutdown::TimedOut { signal } => {
            eprintln!(
                "task did not converge within {}s after signal {signal}, forcing exit; the last position may not have been recorded",
                grace.as_secs()
            );
            shutdown::EXIT_SHUTDOWN_TIMED_OUT
        }
    }
}

/// True when the error is only the shutdown releasing a wait, not a real failure.
fn is_cancellation(e: &anyhow::Error) -> bool {
    e.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<dt_common::error::Error>(),
            Some(dt_common::error::Error::Cancelled(_))
        )
    })
}
