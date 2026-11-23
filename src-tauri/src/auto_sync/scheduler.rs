use chrono::{DateTime, Local, TimeZone};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};
use tracing::{info, warn};

use crate::database::models::SchedulerHealthStatus;
use crate::error::{AppError, AppResult};

const INSTALLED_TASK_NAME: &str = "Browsory_AutoHistorySync";
static TASK_UPDATE_LOCK: Mutex<()> = Mutex::new(());

/// Serializes config writes with the final task refresh performed after a run.
pub fn task_update_guard() -> AppResult<MutexGuard<'static, ()>> {
    TASK_UPDATE_LOCK
        .lock()
        .map_err(|_| AppError::System("自动同步计划任务锁不可用".into()))
}

pub fn task_name(is_portable: bool, instance_id: &str) -> String {
    if !is_portable {
        return INSTALLED_TASK_NAME.to_string();
    }
    let suffix: String = instance_id
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .take(8)
        .collect();
    if suffix.is_empty() {
        INSTALLED_TASK_NAME.to_string()
    } else {
        format!("{}_{}", INSTALLED_TASK_NAME, suffix)
    }
}

pub fn format_local_start_boundary(timestamp_ms: i64) -> String {
    let dt: DateTime<Local> = Local
        .timestamp_millis_opt(timestamp_ms)
        .single()
        .unwrap_or_else(Local::now);
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
}

pub fn xml_escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(ch),
        }
    }
    out
}

pub fn build_task_xml(
    exe_path: &Path,
    working_dir: &Path,
    interval_days: u32,
    start_boundary_ms: i64,
) -> String {
    let exe_escaped = xml_escape(&exe_path.to_string_lossy());
    let dir_escaped = xml_escape(&working_dir.to_string_lossy());
    let start_boundary = format_local_start_boundary(start_boundary_ms);

    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Author>Browsory</Author>
    <Description>Browsory 定期增量归档已绑定的浏览器历史记录</Description>
  </RegistrationInfo>
  <Triggers>
    <CalendarTrigger>
      <StartBoundary>{start_boundary}</StartBoundary>
      <Enabled>true</Enabled>
      <ScheduleByDay>
        <DaysInterval>{interval_days}</DaysInterval>
      </ScheduleByDay>
    </CalendarTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>LeastPrivilege</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT30M</ExecutionTimeLimit>
    <Priority>7</Priority>
    <RestartOnFailure>
      <Interval>PT15M</Interval>
      <Count>3</Count>
    </RestartOnFailure>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe_escaped}</Command>
      <Arguments>--auto-sync --trigger=scheduled</Arguments>
      <WorkingDirectory>{dir_escaped}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

pub fn encode_utf16_le_bom(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(2 + text.len() * 2);
    bytes.push(0xFF);
    bytes.push(0xFE);
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

#[cfg(target_os = "windows")]
fn schtasks_exe_path() -> std::path::PathBuf {
    let windir = std::env::var_os("SystemRoot")
        .or_else(|| std::env::var_os("WINDIR"))
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows"));
    windir.join("System32").join("schtasks.exe")
}

#[cfg(target_os = "windows")]
fn run_schtasks(args: &[&str]) -> AppResult<std::process::Output> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let exe = schtasks_exe_path();
    std::process::Command::new(&exe)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| AppError::System(format!("无法调用 Windows 任务计划程序 ({e})")))
}

#[cfg(target_os = "windows")]
fn decode_windows_output(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xFF, 0xFE]) && bytes.len() >= 2 {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if bytes.starts_with(&[0xFE, 0xFF]) && bytes.len() >= 2 {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    // Check if UTF-16 LE without BOM (every second byte is 0 for ASCII tags)
    if bytes.len() >= 4 && bytes[1] == 0 && bytes[3] == 0 && bytes.len().is_multiple_of(2) {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Ok(utf8) = std::str::from_utf8(bytes) {
        return utf8.to_string();
    }
    // Fallback for localized Windows console/OEM codepage (e.g. CP936 GBK on zh-CN Windows)
    use windows_sys::Win32::Globalization::{MultiByteToWideChar, CP_OEMCP};
    unsafe {
        let needed = MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            bytes.len() as i32,
            std::ptr::null_mut(),
            0,
        );
        if needed > 0 {
            let mut wide = vec![0u16; needed as usize];
            let written = MultiByteToWideChar(
                CP_OEMCP,
                0,
                bytes.as_ptr(),
                bytes.len() as i32,
                wide.as_mut_ptr(),
                needed,
            );
            if written > 0 {
                return String::from_utf16_lossy(&wide[..written as usize]);
            }
        }
    }
    String::from_utf8_lossy(bytes).into_owned()
}

fn extract_xml_tag_value(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml[start..end].trim().to_string())
}

pub fn xml_unescape(input: &str) -> String {
    input
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

pub fn inspect_task_xml_health(
    xml: &str,
    expected_exe: &Path,
    expected_interval_days: u32,
) -> SchedulerHealthStatus {
    let cmd = extract_xml_tag_value(xml, "Command")
        .map(|s| xml_unescape(&s))
        .unwrap_or_default();
    let args = extract_xml_tag_value(xml, "Arguments")
        .map(|s| xml_unescape(&s))
        .unwrap_or_default();
    let interval_ok = extract_xml_tag_value(xml, "DaysInterval")
        .and_then(|s| s.parse::<u32>().ok())
        .is_some_and(|d| d == expected_interval_days);
    let enabled_ok = !xml.contains("<Enabled>false</Enabled>");

    let expected_exe_str = expected_exe.to_string_lossy();
    let path_matches = cmd.eq_ignore_ascii_case(expected_exe_str.as_ref());
    let args_ok = args.contains("--auto-sync");

    if path_matches && args_ok && interval_ok && enabled_ok {
        SchedulerHealthStatus::Registered
    } else {
        SchedulerHealthStatus::OutdatedPath
    }
}

#[cfg(target_os = "windows")]
pub fn create_or_update_task(
    task_name: &str,
    exe_path: &Path,
    temp_dir: &Path,
    interval_days: u32,
    start_boundary_ms: i64,
) -> AppResult<()> {
    let working_dir = exe_path
        .parent()
        .ok_or_else(|| AppError::System("无法定位主程序所在目录".into()))?;
    let xml = build_task_xml(exe_path, working_dir, interval_days, start_boundary_ms);
    let bytes = encode_utf16_le_bom(&xml);

    std::fs::create_dir_all(temp_dir)?;
    let xml_path = temp_dir.join(format!("browsory-task-{}.xml", uuid::Uuid::new_v4()));

    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }
    let _cleanup = Cleanup(xml_path.clone());
    std::fs::write(&xml_path, &bytes)?;

    let xml_str = xml_path.to_string_lossy().to_string();
    let output = run_schtasks(&["/Create", "/TN", task_name, "/XML", &xml_str, "/F"])?;
    if !output.status.success() {
        let stderr = decode_windows_output(&output.stderr);
        let stdout = decode_windows_output(&output.stdout);
        let detail = if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            stdout.trim().to_string()
        };
        warn!(
            task_name,
            detail, "Failed to register Windows scheduled task"
        );
        return Err(AppError::System(format!(
            "注册 Windows 计划任务失败: {}",
            if detail.is_empty() {
                "未知错误".to_string()
            } else {
                detail
            }
        )));
    }
    info!(
        task_name,
        interval_days, "Registered Windows scheduled task for auto history sync"
    );
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn create_or_update_task(
    _task_name: &str,
    _exe_path: &Path,
    _temp_dir: &Path,
    _interval_days: u32,
    _start_boundary_ms: i64,
) -> AppResult<()> {
    Err(AppError::System(
        "当前操作系统不支持 Windows 任务计划程序".into(),
    ))
}

#[cfg(target_os = "windows")]
pub fn delete_task(task_name: &str) -> AppResult<()> {
    let output = run_schtasks(&["/Delete", "/TN", task_name, "/F"])?;
    if output.status.success() {
        info!(task_name, "Deleted Windows scheduled task");
        return Ok(());
    }
    let stderr = decode_windows_output(&output.stderr);
    let stdout = decode_windows_output(&output.stdout);
    let combined = format!("{} {}", stderr, stdout).to_ascii_lowercase();
    // Missing task is already clean
    if combined.contains("cannot find")
        || combined.contains("does not exist")
        || stderr.contains("找不到")
        || stderr.contains("不存在")
    {
        return Ok(());
    }
    Err(AppError::System(format!(
        "删除 Windows 计划任务失败: {}",
        stderr.trim()
    )))
}

#[cfg(not(target_os = "windows"))]
pub fn delete_task(_task_name: &str) -> AppResult<()> {
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn query_task_health(
    task_name: &str,
    expected_exe: &Path,
    expected_interval_days: u32,
) -> (SchedulerHealthStatus, Option<String>) {
    let output = match run_schtasks(&["/Query", "/TN", task_name, "/XML"]) {
        Ok(o) => o,
        Err(e) => return (SchedulerHealthStatus::Error, Some(e.to_string())),
    };
    if !output.status.success() {
        let stderr = decode_windows_output(&output.stderr);
        let lower = stderr.to_ascii_lowercase();
        if lower.contains("cannot find")
            || lower.contains("does not exist")
            || stderr.contains("找不到")
            || stderr.contains("不存在")
        {
            return (SchedulerHealthStatus::Missing, None);
        }
        return (
            SchedulerHealthStatus::Error,
            Some(stderr.trim().to_string()),
        );
    }
    let xml = decode_windows_output(&output.stdout);
    (
        inspect_task_xml_health(&xml, expected_exe, expected_interval_days),
        None,
    )
}

#[cfg(not(target_os = "windows"))]
pub fn query_task_health(
    _task_name: &str,
    _expected_exe: &Path,
    _expected_interval_days: u32,
) -> (SchedulerHealthStatus, Option<String>) {
    (SchedulerHealthStatus::Unsupported, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn task_name_separates_installed_and_portable() {
        assert_eq!(task_name(false, "abcdef123456"), "Browsory_AutoHistorySync");
        assert_eq!(
            task_name(true, "abcdef12-3456"),
            "Browsory_AutoHistorySync_abcdef12"
        );
    }

    #[test]
    fn xml_builder_escapes_special_chars_and_roundtrips_health_check() {
        let exe = PathBuf::from(r"C:\Program Files\Browsory & Tools\Browsory.exe");
        let dir = PathBuf::from(r"C:\Program Files\Browsory & Tools");
        let xml = build_task_xml(&exe, &dir, 30, 1_795_000_000_000);
        assert!(xml.contains("&amp;"));
        assert!(xml.contains("<DaysInterval>30</DaysInterval>"));
        assert!(xml.contains("<StartWhenAvailable>true</StartWhenAvailable>"));
        assert!(xml.contains("<Priority>7</Priority>"));
        assert_eq!(
            inspect_task_xml_health(&xml, &exe, 30),
            SchedulerHealthStatus::Registered
        );
        assert_eq!(
            inspect_task_xml_health(&xml, Path::new(r"D:\Moved\Browsory.exe"), 30),
            SchedulerHealthStatus::OutdatedPath
        );
        assert_eq!(
            inspect_task_xml_health(&xml, &exe, 7),
            SchedulerHealthStatus::OutdatedPath
        );
    }

    #[test]
    fn utf16_bom_encoding_starts_with_ff_fe() {
        let bytes = encode_utf16_le_bom("<?xml");
        assert_eq!(&bytes[0..2], &[0xFF, 0xFE]);
        assert_eq!(bytes.len(), 2 + 5 * 2);
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_schtasks_create_query_path_repair_and_delete_roundtrip() {
        let root =
            std::env::temp_dir().join(format!("browsory_schtasks_e2e_{}", uuid::Uuid::new_v4()));
        let temp_dir = root.join("temp");
        std::fs::create_dir_all(&temp_dir).unwrap();
        let fake_exe_1 = root.join("v1").join("Browsory.exe");
        let fake_exe_2 = root.join("v2").join("Browsory.exe");
        let task = format!(
            "Browsory_AutoHistorySync_Test_{}",
            uuid::Uuid::new_v4().simple()
        );

        struct TaskCleanup(String, std::path::PathBuf);
        impl Drop for TaskCleanup {
            fn drop(&mut self) {
                let _ = delete_task(&self.0);
                let _ = std::fs::remove_dir_all(&self.1);
            }
        }
        let _cleanup = TaskCleanup(task.clone(), root.clone());

        let boundary = chrono::Utc::now().timestamp_millis() + 30 * 86_400_000;
        create_or_update_task(&task, &fake_exe_1, &temp_dir, 30, boundary).unwrap();

        let (health_ok, err_ok) = query_task_health(&task, &fake_exe_1, 30);
        assert_eq!(err_ok, None);
        assert_eq!(health_ok, SchedulerHealthStatus::Registered);

        let (health_moved, _) = query_task_health(&task, &fake_exe_2, 30);
        assert_eq!(health_moved, SchedulerHealthStatus::OutdatedPath);

        // Repair to new path and interval
        create_or_update_task(&task, &fake_exe_2, &temp_dir, 7, boundary).unwrap();
        let (health_repaired, _) = query_task_health(&task, &fake_exe_2, 7);
        assert_eq!(health_repaired, SchedulerHealthStatus::Registered);

        delete_task(&task).unwrap();
        let (health_deleted, _) = query_task_health(&task, &fake_exe_2, 7);
        assert_eq!(health_deleted, SchedulerHealthStatus::Missing);
        // Deleting again must remain idempotent
        delete_task(&task).unwrap();
    }
}
