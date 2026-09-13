pub mod detector;

pub use detector::{
    is_process_running, terminate_and_wait, terminate_process, wait_for_process_exit,
};
