use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use chrono::Local;
use log::{LevelFilter, Log, Metadata, Record};
use tauri::{AppHandle, Manager};

const FILE_LIMIT: u64 = 10 * 1024 * 1024;
static LOGGER: OnceLock<RotatingLogger> = OnceLock::new();

struct LogFile {
    file: Option<File>,
    bytes: u64,
}

struct RotatingLogger {
    path: PathBuf,
    state: Mutex<LogFile>,
}

impl Log for RotatingLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let line = format!(
            "{} {:<5} {}\n",
            Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
            record.level(),
            record.args()
        );
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        if state.bytes.saturating_add(line.len() as u64) > FILE_LIMIT {
            if let Some(file) = state.file.as_mut() {
                let _ = file.flush();
            }
            state.file.take();
            let previous = self.path.with_extension("log.1");
            let _ = fs::remove_file(&previous);
            let _ = fs::rename(&self.path, &previous);
            if let Ok(file) = open_log_file(&self.path) {
                state.file = Some(file);
                state.bytes = 0;
            }
        }
        if state
            .file
            .as_mut()
            .is_some_and(|file| file.write_all(line.as_bytes()).is_ok())
        {
            state.bytes = state.bytes.saturating_add(line.len() as u64);
        }
    }

    fn flush(&self) {
        if let Ok(mut state) = self.state.lock() {
            if let Some(file) = state.file.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

pub struct OperationTimer {
    operation: &'static str,
    started: Instant,
}

impl OperationTimer {
    pub fn new(operation: &'static str) -> Self {
        log::info!("operation={operation} event=start");
        Self {
            operation,
            started: Instant::now(),
        }
    }
}

impl Drop for OperationTimer {
    fn drop(&mut self) {
        log::info!(
            "operation={} event=finish duration_ms={}",
            self.operation,
            self.started.elapsed().as_millis()
        );
    }
}

pub fn init(app: &AppHandle) -> Result<PathBuf, String> {
    let directory = app
        .path()
        .app_log_dir()
        .map_err(|error| format!("无法确定日志目录：{error}"))?;
    fs::create_dir_all(&directory).map_err(|error| format!("无法创建日志目录：{error}"))?;
    let path = directory.join("gitsynctools.log");
    let file = open_log_file(&path).map_err(|error| format!("无法创建日志文件：{error}"))?;
    let bytes = file.metadata().map(|metadata| metadata.len()).unwrap_or(0);
    let logger = RotatingLogger {
        path: path.clone(),
        state: Mutex::new(LogFile {
            file: Some(file),
            bytes,
        }),
    };
    if LOGGER.set(logger).is_ok() {
        let logger = LOGGER
            .get()
            .ok_or_else(|| "无法初始化日志模块".to_string())?;
        log::set_logger(logger).map_err(|error| format!("无法启用日志模块：{error}"))?;
        log::set_max_level(LevelFilter::Info);
    }
    log::info!(
        "event=application_started version={} platform={}",
        app.package_info().version,
        std::env::consts::OS
    );
    Ok(path)
}

pub fn log_directory(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_log_dir()
        .map_err(|error| format!("无法确定日志目录：{error}"))
}

pub fn open_log_directory(app: &AppHandle) -> Result<(), String> {
    let directory = log_directory(app)?;
    fs::create_dir_all(&directory).map_err(|error| format!("无法创建日志目录：{error}"))?;
    open_directory(&directory)
}

fn open_log_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new().create(true).append(true).open(path)
}

pub(crate) fn open_directory(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(path);
        command
    };
    #[cfg(target_os = "windows")]
    let mut command = {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let mut command = Command::new("explorer.exe");
        command.arg(path).creation_flags(CREATE_NO_WINDOW);
        command
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(path);
        command
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("无法打开目录：{error}"))?;
    Ok(())
}

pub fn safe_detail(value: &str) -> String {
    let mut result: String = value.chars().take(600).collect();
    if let Some(scheme) = result.find("://") {
        let authority_start = scheme + 3;
        if let Some(at) = result[authority_start..].find('@') {
            result.replace_range(authority_start..authority_start + at, "<credentials>");
        }
    }
    result.replace(['\r', '\n'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_credentials_and_line_breaks_from_diagnostics() {
        let detail = safe_detail("request https://user:token@example.com/a\nfailed");
        assert!(!detail.contains("token"));
        assert!(!detail.contains('\n'));
        assert!(detail.contains("<credentials>@example.com"));
    }
}
