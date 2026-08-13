//! Signal handling for `dt-main`.
//!
//! A running task must not be torn down mid-batch: extractors have to stop, the buffer has to
//! drain and the final position has to be recorded before the process leaves. So a signal does not
//! kill the process — it cancels the task-wide [`CancellationToken`] (see ADR 0002) and then waits
//! a bounded window for the task to converge. The window is the last resort, not the mechanism.
//!
//! Exiting is never silent: an interrupted run leaves with a non-zero code, so an orchestrator
//! cannot mistake "stopped by SIGTERM half way through" for "finished".

use std::future::Future;
use std::time::Duration;

use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

pub const ENV_SHUTDOWN_TIMEOUT_SECS: &str = "SHUTDOWN_TIMEOUT_SECS";

/// Long enough for a pipeline to drain and record its position, short enough to stay inside the
/// console executor's 10s SIGTERM grace window (`CONSOLE_STOP_GRACE_SECS`) so the graceful path
/// wins the race against SIGKILL.
pub const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 8;

/// The task was still running when the graceful window expired and had to be abandoned.
pub const EXIT_SHUTDOWN_TIMED_OUT: i32 = 4;

pub const SIGINT: i32 = 2;
pub const SIGTERM: i32 = 15;

/// How a supervised task ended.
#[derive(Debug)]
pub enum Shutdown<T> {
    /// The task finished on its own; no signal was involved.
    Completed(T),
    /// A signal arrived and the task converged inside the graceful window.
    Interrupted { signal: i32, result: T },
    /// A signal arrived and the task was still running when the window expired.
    TimedOut { signal: i32 },
}

/// `128 + signal`, the shell/`ExitStatus` convention the console already speaks
/// (`dt-console-server::run_handlers` maps a signalled child to the same number).
pub fn exit_code_for_signal(signal: i32) -> i32 {
    128 + signal
}

/// Parse the graceful window. Anything unparsable falls back to the default rather than failing
/// the run: a bad env var must not be the reason a shutdown turns into a hard kill.
pub fn parse_shutdown_timeout(raw: Option<&str>) -> Duration {
    let secs = raw
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_SHUTDOWN_TIMEOUT_SECS);
    Duration::from_secs(secs)
}

pub fn shutdown_timeout_from_env() -> Duration {
    parse_shutdown_timeout(std::env::var(ENV_SHUTDOWN_TIMEOUT_SECS).ok().as_deref())
}

/// Drive `task` until it finishes, or until a signal arrives and the graceful window expires.
///
/// The token is cancelled by the signal listener, not here: by the time a signal shows up on
/// `signals`, the task has already been asked to stop, and this only bounds how long it may take.
pub async fn supervise<F: Future>(
    task: F,
    signals: &mut watch::Receiver<Option<i32>>,
    grace: Duration,
) -> Shutdown<F::Output> {
    tokio::pin!(task);

    let signal = tokio::select! {
        // A task that finishes in the same breath as the signal is still an interrupted run:
        // re-read the channel rather than let the select's coin flip decide the exit code.
        output = &mut task => {
            return match *signals.borrow_and_update() {
                Some(signal) => Shutdown::Interrupted { signal, result: output },
                None => Shutdown::Completed(output),
            }
        }
        signal = next_signal(signals) => signal,
    };

    match tokio::time::timeout(grace, &mut task).await {
        Ok(result) => Shutdown::Interrupted { signal, result },
        Err(_) => Shutdown::TimedOut { signal },
    }
}

/// Resolve with the first signal seen on the channel; never resolve if the sender is gone
/// (no listener means no signal will ever come, so the task side of the select must win).
async fn next_signal(signals: &mut watch::Receiver<Option<i32>>) -> i32 {
    loop {
        if let Some(signal) = *signals.borrow_and_update() {
            return signal;
        }
        if signals.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Listen for termination signals; the first one cancels `cancel_token` and is published on the
/// returned channel, a second one of any kind exits immediately (the operator asked twice).
#[cfg(unix)]
pub fn spawn_signal_listener(cancel_token: CancellationToken) -> watch::Receiver<Option<i32>> {
    use tokio::signal::unix::{signal, Signal, SignalKind};

    /// Installing a handler is irreversible and removes the default terminate behaviour, so a
    /// half-installed listener must keep the half that worked: dropping both would leave the
    /// process with neither a handler nor a default action, i.e. unkillable short of SIGKILL.
    fn install(kind: SignalKind, name: &str) -> Option<Signal> {
        match signal(kind) {
            Ok(stream) => Some(stream),
            Err(e) => {
                eprintln!("failed to install the {name} handler, {name} will not stop this task gracefully: {e}");
                None
            }
        }
    }

    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        let mut sigint = install(SignalKind::interrupt(), "SIGINT");
        let mut sigterm = install(SignalKind::terminate(), "SIGTERM");

        loop {
            let received = match (sigint.as_mut(), sigterm.as_mut()) {
                (Some(sigint), Some(sigterm)) => tokio::select! {
                    _ = sigint.recv() => SIGINT,
                    _ = sigterm.recv() => SIGTERM,
                },
                (Some(sigint), None) => {
                    sigint.recv().await;
                    SIGINT
                }
                (None, Some(sigterm)) => {
                    sigterm.recv().await;
                    SIGTERM
                }
                (None, None) => {
                    eprintln!("no signal handler could be installed; this task cannot be stopped gracefully");
                    return;
                }
            };
            handle_signal(received, &tx, &cancel_token);
        }
    });
    rx
}

/// Windows has no SIGTERM; ctrl-c is the only cooperative stop available. Note that each
/// `ctrl_c()` registration starts with no pending event, so a second ctrl-c landing between two
/// iterations can be missed — the "press it again to exit now" escape hatch is best effort here.
#[cfg(not(unix))]
pub fn spawn_signal_listener(cancel_token: CancellationToken) -> watch::Receiver<Option<i32>> {
    let (tx, rx) = watch::channel(None);
    tokio::spawn(async move {
        while tokio::signal::ctrl_c().await.is_ok() {
            handle_signal(SIGINT, &tx, &cancel_token);
        }
    });
    rx
}

fn handle_signal(signal: i32, tx: &watch::Sender<Option<i32>>, cancel_token: &CancellationToken) {
    if tx.borrow().is_none() {
        eprintln!(
            "received signal {signal}, stopping the task gracefully (window: {}s, override with {}); send it again to exit immediately",
            shutdown_timeout_from_env().as_secs(),
            ENV_SHUTDOWN_TIMEOUT_SECS
        );
        let _ = tx.send(Some(signal));
        cancel_token.cancel();
    } else {
        eprintln!("received signal {signal} while already shutting down, exiting immediately");
        std::process::exit(exit_code_for_signal(signal));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_exit_codes_follow_the_128_plus_signal_convention() {
        assert_eq!(exit_code_for_signal(SIGINT), 130);
        assert_eq!(exit_code_for_signal(SIGTERM), 143);
        assert_ne!(exit_code_for_signal(SIGTERM), 0);
    }

    #[test]
    fn a_missing_or_broken_timeout_falls_back_to_the_default() {
        let default = Duration::from_secs(DEFAULT_SHUTDOWN_TIMEOUT_SECS);
        assert_eq!(parse_shutdown_timeout(None), default);
        assert_eq!(parse_shutdown_timeout(Some("")), default);
        assert_eq!(parse_shutdown_timeout(Some("soon")), default);
        assert_eq!(parse_shutdown_timeout(Some("-1")), default);
    }

    #[test]
    fn an_explicit_timeout_is_honoured_including_zero() {
        assert_eq!(parse_shutdown_timeout(Some("12")), Duration::from_secs(12));
        assert_eq!(
            parse_shutdown_timeout(Some(" 12 ")),
            Duration::from_secs(12)
        );
        assert_eq!(parse_shutdown_timeout(Some("0")), Duration::ZERO);
    }

    #[tokio::test]
    async fn a_task_that_finishes_first_is_not_reported_as_interrupted() {
        let (_tx, mut rx) = watch::channel(None);
        let outcome = supervise(async { 7 }, &mut rx, Duration::from_secs(60)).await;
        assert!(matches!(outcome, Shutdown::Completed(7)), "{outcome:?}");
    }

    #[tokio::test]
    async fn a_signal_lets_the_task_converge_inside_the_window() {
        let cancel_token = CancellationToken::new();
        let (tx, mut rx) = watch::channel(None);
        let task_token = cancel_token.clone();
        let task = async move {
            task_token.cancelled().await;
            "drained"
        };

        handle_signal(SIGTERM, &tx, &cancel_token);
        let outcome = supervise(task, &mut rx, Duration::from_secs(60)).await;

        match outcome {
            Shutdown::Interrupted { signal, result } => {
                assert_eq!(signal, SIGTERM);
                assert_eq!(result, "drained");
            }
            other => panic!("expected an interrupted shutdown, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_task_that_ignores_the_signal_times_out_instead_of_hanging() {
        let cancel_token = CancellationToken::new();
        let (tx, mut rx) = watch::channel(None);

        handle_signal(SIGINT, &tx, &cancel_token);
        assert!(
            cancel_token.is_cancelled(),
            "the signal must cancel the task"
        );

        let outcome = supervise(
            std::future::pending::<()>(),
            &mut rx,
            Duration::from_millis(50),
        )
        .await;

        assert!(
            matches!(outcome, Shutdown::TimedOut { signal: SIGINT }),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn a_task_that_finishes_as_the_signal_lands_still_reports_the_signal() {
        let cancel_token = CancellationToken::new();
        let (tx, mut rx) = watch::channel(None);
        handle_signal(SIGTERM, &tx, &cancel_token);

        // The task future is ready on its first poll, so the select could pick either branch;
        // the exit code must not depend on which one it picked.
        let outcome = supervise(std::future::ready(()), &mut rx, Duration::from_secs(60)).await;

        assert!(
            matches!(
                outcome,
                Shutdown::Interrupted {
                    signal: SIGTERM,
                    ..
                }
            ),
            "{outcome:?}"
        );
    }

    #[tokio::test]
    async fn a_signal_that_arrives_mid_run_still_stops_the_task() {
        let cancel_token = CancellationToken::new();
        let (tx, mut rx) = watch::channel(None);
        let task_token = cancel_token.clone();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            handle_signal(SIGTERM, &tx, &cancel_token);
        });

        let outcome = supervise(
            async move {
                task_token.cancelled().await;
            },
            &mut rx,
            Duration::from_secs(60),
        )
        .await;

        assert!(
            matches!(
                outcome,
                Shutdown::Interrupted {
                    signal: SIGTERM,
                    ..
                }
            ),
            "{outcome:?}"
        );
    }
}
