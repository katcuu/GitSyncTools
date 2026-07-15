use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, State};

use crate::git::{GitRepository, RemoteInspection};
use crate::model::{
    AppConfig, ApplyPullInput, ConfigureInput, DeleteRepositoryFilesInput, DeviceRole, LocalState,
    PublishResult, PullPlan, RepositorySnapshot, SyncStatus, ValidateInput,
};
use crate::storage::{load_config, load_state, save_config, save_state, RuntimePaths};
use crate::sync::{
    apply_pull_plan, delete_repository_files as delete_files, prepare_pull_plan, publish_files,
    read_repository_snapshot, refresh_repository_snapshot, retry_pending_push as retry_push,
};

#[derive(Default)]
pub struct AppState {
    pub operation: Mutex<()>,
    pub close_hint_shown: AtomicBool,
    pub tray_available: AtomicBool,
    pub repository_loading: AtomicBool,
    validation_cache: Mutex<Option<CachedRemote>>,
}

pub enum TraySyncOutcome {
    Completed(String),
    NeedsAttention(String),
}

#[derive(Clone)]
struct CachedRemote {
    repository_url: String,
    branch: String,
    head: Option<String>,
    checked_at: Instant,
}

impl AppState {
    fn cached_remote(
        &self,
        repository_url: &str,
        branch: &str,
    ) -> Result<Option<Option<String>>, String> {
        let cache = self
            .validation_cache
            .lock()
            .map_err(|_| "连接检测缓存已损坏".to_string())?;
        Ok(cache.as_ref().and_then(|cached| {
            (cached.repository_url == repository_url
                && cached.branch == branch
                && cached.checked_at.elapsed() <= Duration::from_secs(120))
            .then(|| cached.head.clone())
        }))
    }

    fn remember_remote(
        &self,
        repository_url: &str,
        branch: &str,
        inspection: &RemoteInspection,
    ) -> Result<(), String> {
        let mut cache = self
            .validation_cache
            .lock()
            .map_err(|_| "连接检测缓存已损坏".to_string())?;
        *cache = Some(CachedRemote {
            repository_url: repository_url.to_owned(),
            branch: branch.to_owned(),
            head: inspection.head.clone(),
            checked_at: Instant::now(),
        });
        Ok(())
    }
}

#[tauri::command]
pub fn get_sync_status(app: AppHandle) -> Result<SyncStatus, String> {
    current_status(&app)
}

#[tauri::command]
pub fn clear_last_error(app: AppHandle) -> Result<(), String> {
    let runtime = RuntimePaths::from_app(&app)?;
    let mut state = load_state(&runtime)?;
    state.last_error = None;
    save_state(&runtime, &state)?;
    let _ = app.emit("sync-status-updated", ());
    Ok(())
}

pub fn clear_startup_error(app: &AppHandle) -> Result<(), String> {
    let runtime = RuntimePaths::from_app(app)?;
    let mut state = load_state(&runtime)?;
    if state.last_error.is_some() && !state.pending_push {
        state.last_error = None;
        save_state(&runtime, &state)?;
    }
    Ok(())
}

#[tauri::command]
pub fn get_update_proxy() -> Option<String> {
    crate::proxy::update_proxy()
}

#[tauri::command]
pub fn open_log_directory(app: AppHandle) -> Result<(), String> {
    crate::diagnostics::open_log_directory(&app)
}

#[tauri::command]
pub fn record_update_event(stage: String, detail: Option<String>, duration_ms: u64) {
    let valid_stage = stage
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '_');
    if !valid_stage || stage.len() > 40 {
        return;
    }
    let detail = detail
        .as_deref()
        .map(crate::diagnostics::safe_detail)
        .unwrap_or_default();
    log::info!(
        "operation=software_update stage={} duration_ms={} detail={}",
        stage,
        duration_ms,
        detail
    );
}

#[tauri::command]
pub fn get_repository_snapshot(
    app: AppHandle,
    shared: State<'_, AppState>,
) -> Result<RepositorySnapshot, String> {
    if shared.repository_loading.load(Ordering::SeqCst) {
        return Ok(RepositorySnapshot::empty(
            "正在后台读取仓库文件，完成后会自动刷新",
        ));
    }
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let Some(config) = load_config(&runtime)? else {
        return Ok(RepositorySnapshot::empty("尚未配置仓库"));
    };
    read_repository_snapshot(&runtime, &config)
}

#[tauri::command]
pub fn refresh_repository(
    app: AppHandle,
    shared: State<'_, AppState>,
) -> Result<RepositorySnapshot, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    match refresh_repository_snapshot(&runtime, &config, &mut local) {
        Ok(snapshot) => {
            let _ = app.emit("sync-status-updated", ());
            Ok(snapshot)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            let _ = app.emit("sync-status-updated", ());
            Err(error)
        }
    }
}

#[tauri::command]
pub fn validate_connection(
    shared: State<'_, AppState>,
    input: ValidateInput,
) -> Result<String, String> {
    reject_embedded_credentials(&input.repository_url)?;
    let repository_url = input.repository_url.trim();
    let branch = input.branch.trim();
    let inspection = GitRepository::inspect_connection(repository_url, branch)?;
    shared.remember_remote(repository_url, branch, &inspection)?;
    Ok(inspection.message)
}

#[tauri::command]
pub fn configure_repository(
    app: AppHandle,
    shared: State<'_, AppState>,
    input: ConfigureInput,
) -> Result<AppConfig, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let previous = load_config(&runtime)?;
    let previous_state = load_state(&runtime)?;
    if previous_state.pending_push {
        return Err("存在尚未上传的内容，不能更换连接设置".into());
    }

    let repository_url = input.repository_url.trim().to_owned();
    let branch = input.branch.trim().to_owned();
    reject_embedded_credentials(&repository_url)?;
    let cached_remote = shared.cached_remote(&repository_url, &branch)?;
    let remote = match cached_remote {
        Some(head) => head,
        None => {
            let inspection = GitRepository::inspect_connection(&repository_url, &branch)?;
            shared.remember_remote(&repository_url, &branch, &inspection)?;
            inspection.head
        }
    };
    let destination = match input.role {
        DeviceRole::Receiver => {
            let path = input
                .destination
                .ok_or_else(|| "接收端必须选择同步目录".to_string())?;
            fs::create_dir_all(&path).map_err(|error| format!("无法创建同步目录：{error}"))?;
            if !path.is_dir() {
                return Err("同步目录不是有效文件夹".into());
            }
            Some(dunce::canonicalize(&path).map_err(|error| format!("无法访问同步目录：{error}"))?)
        }
        DeviceRole::Sender => None,
    };
    let config = AppConfig {
        repository_url,
        branch,
        role: input.role,
        destination,
    };

    let repository_changed = previous
        .as_ref()
        .map(|old| old.repository_url != config.repository_url || old.branch != config.branch)
        .unwrap_or(true);
    let local_mapping_changed = previous
        .as_ref()
        .map(|old| old.role != config.role || old.destination != config.destination)
        .unwrap_or(true);
    if repository_changed && runtime.repository.exists() {
        fs::remove_dir_all(&runtime.repository)
            .map_err(|error| format!("无法重建内部仓库：{error}"))?;
    }

    let repository = GitRepository::new(runtime.repository.clone(), &config)?;
    repository.ensure()?;
    let needs_repository_load = match remote.as_deref() {
        Some(commit) => repository.head()?.as_deref() != Some(commit),
        None => false,
    };

    let mut next_state = if repository_changed || local_mapping_changed {
        LocalState::default()
    } else {
        previous_state
    };
    next_state.last_remote_commit = remote.clone();
    next_state.last_error = None;
    save_config(&runtime, &config)?;
    save_state(&runtime, &next_state)?;
    let _ = app.emit("sync-status-updated", ());
    if needs_repository_load {
        if let Some(commit) = remote {
            start_repository_load(app.clone(), config.clone(), commit);
        }
    }
    Ok(config)
}

fn start_repository_load(app: AppHandle, config: AppConfig, commit: String) {
    let shared = app.state::<AppState>();
    if shared.repository_loading.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = app.emit("sync-status-updated", ());
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let shared = app.state::<AppState>();
            let _operation = shared
                .operation
                .lock()
                .map_err(|_| "同步状态锁已损坏".to_string())?;
            let runtime = RuntimePaths::from_app(&app)?;
            let repository = GitRepository::new(runtime.repository.clone(), &config)?;
            repository.ensure()?;
            repository.checkout_remote(&commit)?;
            let mut state = load_state(&runtime)?;
            state.last_remote_commit = Some(commit);
            state.last_error = None;
            save_state(&runtime, &state)
        })();

        if let Err(error) = result {
            if let Ok(runtime) = RuntimePaths::from_app(&app) {
                if let Ok(mut state) = load_state(&runtime) {
                    remember_error(&runtime, &config, &mut state, &error);
                }
            }
        }
        app.state::<AppState>()
            .repository_loading
            .store(false, Ordering::SeqCst);
        let _ = app.emit("sync-status-updated", ());
    });
}

#[tauri::command]
pub fn publish(
    app: AppHandle,
    shared: State<'_, AppState>,
    paths: Vec<PathBuf>,
) -> Result<PublishResult, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    publish_locked(&app, paths)
}

#[tauri::command]
pub fn delete_repository_files(
    app: AppHandle,
    shared: State<'_, AppState>,
    input: DeleteRepositoryFilesInput,
) -> Result<PublishResult, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    match delete_files(
        &runtime,
        &config,
        &mut local,
        input.entries,
        input.expected_commit.as_deref(),
    ) {
        Ok(result) => {
            let _ = app.emit("sync-status-updated", ());
            Ok(result)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            let _ = app.emit("sync-status-updated", ());
            Err(error)
        }
    }
}

#[tauri::command]
pub fn retry_pending_push(
    app: AppHandle,
    shared: State<'_, AppState>,
) -> Result<PublishResult, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    match retry_push(&runtime, &config, &mut local) {
        Ok(result) => {
            let _ = app.emit("sync-status-updated", ());
            Ok(result)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn prepare_pull(app: AppHandle, shared: State<'_, AppState>) -> Result<PullPlan, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    match prepare_pull_plan(&runtime, &config, &mut local) {
        Ok(plan) => {
            let _ = app.emit("sync-status-updated", ());
            Ok(plan)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn apply_pull(
    app: AppHandle,
    shared: State<'_, AppState>,
    input: ApplyPullInput,
) -> Result<SyncStatus, String> {
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(&app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    match apply_pull_plan(
        &runtime,
        &config,
        &mut local,
        &input.commit,
        &input.resolutions,
    ) {
        Ok(()) => {
            let _ = app.emit("sync-status-updated", ());
            current_status(&app)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            Err(error)
        }
    }
}

pub fn publish_for_context(app: &AppHandle, paths: Vec<PathBuf>) -> Result<PublishResult, String> {
    let shared = app.state::<AppState>();
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    publish_locked(app, paths)
}

pub fn sync_from_tray(app: &AppHandle) -> Result<TraySyncOutcome, String> {
    let shared = app.state::<AppState>();
    let _operation = shared
        .operation
        .lock()
        .map_err(|_| "同步状态锁已损坏".to_string())?;
    let runtime = RuntimePaths::from_app(app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    let result = (|| -> Result<TraySyncOutcome, String> {
        match config.role {
            DeviceRole::Sender => refresh_repository_snapshot(&runtime, &config, &mut local)
                .map(|_| TraySyncOutcome::Completed("仓库信息已更新".into())),
            DeviceRole::Receiver => {
                let plan = prepare_pull_plan(&runtime, &config, &mut local)?;
                if !plan.conflicts.is_empty() {
                    Ok(TraySyncOutcome::NeedsAttention(format!(
                        "检测到 {} 个本地文件冲突，请在主窗口处理",
                        plan.conflicts.len()
                    )))
                } else if let Some(commit) = plan.commit.as_deref() {
                    apply_pull_plan(&runtime, &config, &mut local, commit, &[])?;
                    let message = if plan.changes.is_empty() {
                        "已是最新版本"
                    } else {
                        "接收目录已更新"
                    };
                    Ok(TraySyncOutcome::Completed(message.into()))
                } else {
                    Ok(TraySyncOutcome::Completed(plan.message))
                }
            }
        }
    })();
    match result {
        Ok(outcome) => {
            let _ = app.emit("sync-status-updated", ());
            Ok(outcome)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            let _ = app.emit("sync-status-updated", ());
            Err(error)
        }
    }
}

fn publish_locked(app: &AppHandle, paths: Vec<PathBuf>) -> Result<PublishResult, String> {
    let runtime = RuntimePaths::from_app(app)?;
    let config = required_config(&runtime)?;
    let mut local = load_state(&runtime)?;
    match publish_files(&runtime, &config, &mut local, paths) {
        Ok(result) => {
            let _ = app.emit("sync-status-updated", ());
            Ok(result)
        }
        Err(error) => {
            remember_error(&runtime, &config, &mut local, &error);
            let _ = app.emit("sync-status-updated", ());
            Err(error)
        }
    }
}

fn required_config(runtime: &RuntimePaths) -> Result<AppConfig, String> {
    load_config(runtime)?.ok_or_else(|| "请先完成连接设置".into())
}

fn current_status(app: &AppHandle) -> Result<SyncStatus, String> {
    let runtime = RuntimePaths::from_app(app)?;
    let config = load_config(&runtime)?;
    let state = load_state(&runtime)?;
    let repository_loading = app
        .state::<AppState>()
        .repository_loading
        .load(Ordering::SeqCst);
    let phase = if repository_loading {
        "working"
    } else if state.pending_push {
        "pendingPush"
    } else if state.last_error.is_some() {
        "error"
    } else {
        "idle"
    };
    Ok(SyncStatus {
        configured: config.is_some(),
        platform: std::env::consts::OS.to_owned(),
        app_version: app.package_info().version.to_string(),
        config,
        phase: phase.into(),
        repository_loading,
        last_sync_at: state.last_sync_at,
        last_checked_at: state.last_checked_at,
        pending_push: state.pending_push,
        pending_commit: state.pending_commit,
        last_error: state.last_error,
        last_applied_commit: state.last_applied_commit,
    })
}

fn remember_error(runtime: &RuntimePaths, config: &AppConfig, state: &mut LocalState, error: &str) {
    let mut sanitized = error.replace(&runtime.root.to_string_lossy().to_string(), "<app-data>");
    if let Some(destination) = &config.destination {
        sanitized = sanitized.replace(&destination.to_string_lossy().to_string(), "<sync-folder>");
    }
    state.last_error = Some(sanitized);
    let _ = save_state(runtime, state);
}

fn reject_embedded_credentials(url: &str) -> Result<(), String> {
    if url.trim_start().starts_with('-') {
        return Err("仓库地址无效".into());
    }
    if let Some(scheme) = url.find("://") {
        let protocol = url[..scheme].to_ascii_lowercase();
        let remainder = &url[scheme + 3..];
        if matches!(protocol.as_str(), "http" | "https")
            && remainder
                .split('/')
                .next()
                .is_some_and(|authority| authority.contains('@'))
        {
            return Err("仓库地址不能包含账号或令牌，请使用系统凭据管理器".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_https_credentials() {
        assert!(reject_embedded_credentials("https://example.com/repo.git").is_ok());
        assert!(reject_embedded_credentials("git@example.com:repo.git").is_ok());
        assert!(reject_embedded_credentials("ssh://git@example.com/repo.git").is_ok());
        assert!(reject_embedded_credentials("https://user:token@example.com/repo.git").is_err());
        assert!(reject_embedded_credentials("--upload-pack=bad").is_err());
    }

    #[test]
    fn reuses_a_recent_connection_check_for_the_same_repository() {
        let state = AppState::default();
        let inspection = RemoteInspection {
            head: Some("abc123".into()),
            message: "连接成功".into(),
        };

        state
            .remember_remote("https://example.com/repo.git", "main", &inspection)
            .unwrap();

        assert_eq!(
            state
                .cached_remote("https://example.com/repo.git", "main")
                .unwrap(),
            Some(Some("abc123".into()))
        );
        assert_eq!(
            state
                .cached_remote("https://example.com/repo.git", "dev")
                .unwrap(),
            None
        );
    }
}
