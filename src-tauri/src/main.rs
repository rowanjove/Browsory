// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(exit_code) = app_lib::auto_sync::cli::try_run_headless_from_env() {
        std::process::exit(exit_code);
    }
    app_lib::run();
}
