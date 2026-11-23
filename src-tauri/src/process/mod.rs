pub mod detector;
pub mod jobs;

pub use detector::{
    is_process_running, running_process_names, terminate_and_wait, terminate_process,
    wait_for_process_exit,
};
pub use jobs::BackgroundJob;
