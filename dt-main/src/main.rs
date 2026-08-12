use std::env;

use dt_precheck::{config::task_config::PrecheckTaskConfig, do_precheck, load_precheck_configs};
use dt_task::task_runner::TaskRunner;

const ENV_SHUTDOWN_TIMEOUT_SECS: &str = "SHUTDOWN_TIMEOUT_SECS";

#[tokio::main]
async fn main() {
    env::set_var("RUST_BACKTRACE", "1");

    tokio::spawn(async {
        tokio::signal::ctrl_c().await.unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(
            std::env::var(ENV_SHUTDOWN_TIMEOUT_SECS)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
        ))
        .await;
        std::process::exit(0);
    });

    let task_config = env::args().nth(1).expect("no task_config provided in args");
    let is_precheck = match PrecheckTaskConfig::is_precheck(&task_config) {
        Ok(is_precheck) => is_precheck,
        Err(e) => {
            eprintln!("task config init failed: {e:#}");
            std::process::exit(2);
        }
    };
    if is_precheck {
        let (precheck_config, task_config) = match load_precheck_configs(&task_config) {
            Ok(configs) => configs,
            Err(e) => {
                eprintln!("precheck init failed: {e:#}");
                std::process::exit(2);
            }
        };
        if let Err(e) = do_precheck(precheck_config, task_config).await {
            eprintln!("precheck failed: {e:#}");
            std::process::exit(3);
        }
    } else {
        let runner = match TaskRunner::new(&task_config) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("task runner init failed: {e:#}");
                std::process::exit(2);
            }
        };
        if let Err(e) = runner.start_task().await {
            eprintln!("task execution failed: {e:#}");
            std::process::exit(3);
        }
    }
}
