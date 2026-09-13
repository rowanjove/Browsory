use std::process::Command;
use sysinfo::{ProcessesToUpdate, System};
use tracing::{info, warn};

pub fn is_process_running(process_name: &str) -> bool {
    let mut sys = System::new();
    sys.refresh_processes(ProcessesToUpdate::All, true);

    let target = process_name.to_lowercase();
    for proc_ in sys.processes().values() {
        let name = proc_.name().to_string_lossy().to_lowercase();
        if name == target {
            return true;
        }
    }
    false
}

pub fn terminate_process(process_name: &str, force: bool) -> Result<(), String> {
    info!("Terminating process: {} (force={})", process_name, force);

    let mut cmd = Command::new("taskkill");
    if force {
        cmd.args(["/F", "/IM", process_name]);
    } else {
        cmd.args(["/IM", process_name]);
    }

    match cmd.output() {
        Ok(output) => {
            if output.status.success() {
                info!("Successfully terminated {}", process_name);
                Ok(())
            } else {
                let err_msg = String::from_utf8_lossy(&output.stderr).to_string();
                warn!("Failed to terminate {}: {}", process_name, err_msg);
                Err(err_msg)
            }
        }
        Err(e) => Err(format!("Failed to execute taskkill: {}", e)),
    }
}

pub fn wait_for_process_exit(process_name: &str, timeout_ms: u64) -> bool {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let check_interval = std::time::Duration::from_millis(200);

    while start.elapsed() < timeout {
        if !is_process_running(process_name) {
            return true;
        }
        std::thread::sleep(check_interval);
    }
    !is_process_running(process_name)
}

pub fn terminate_and_wait(process_name: &str, force: bool, timeout_ms: u64) -> Result<(), String> {
    terminate_process(process_name, force)?;
    if wait_for_process_exit(process_name, timeout_ms) {
        Ok(())
    } else {
        Err(format!("Timed out waiting for {} to exit", process_name))
    }
}
