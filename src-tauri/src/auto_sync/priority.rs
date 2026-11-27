#[cfg(target_os = "windows")]
pub fn apply_background_priority() {
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS,
        PROCESS_MODE_BACKGROUND_BEGIN,
    };

    unsafe {
        let handle = GetCurrentProcess();
        // PROCESS_MODE_BACKGROUND_BEGIN lowers CPU, I/O, and memory page priority.
        if SetPriorityClass(handle, PROCESS_MODE_BACKGROUND_BEGIN) == 0 {
            let _ = SetPriorityClass(handle, BELOW_NORMAL_PRIORITY_CLASS);
        }
    }
}

#[cfg(not(target_os = "windows"))]
pub fn apply_background_priority() {}
