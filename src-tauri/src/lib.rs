mod commands;
mod diagnostics;
mod git;
mod model;
mod proxy;
mod storage;
mod sync;

use std::path::PathBuf;

use commands::{
    apply_pull, clear_last_error, configure_repository, delete_repository_files,
    get_repository_snapshot, get_sync_status, get_update_proxy, open_log_directory, prepare_pull,
    publish, record_update_event, refresh_repository, retry_pending_push, validate_connection,
    AppState,
};
use std::sync::atomic::Ordering;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, WindowEvent};
use tauri_plugin_notification::NotificationExt;

fn context_paths(arguments: &[String]) -> Vec<PathBuf> {
    arguments
        .iter()
        .position(|argument| argument == "--publish")
        .map(|index| arguments[index + 1..].iter().map(PathBuf::from).collect())
        .unwrap_or_default()
}

fn show_main(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn create_tray(app: &tauri::App) -> Result<(), String> {
    let show = MenuItem::with_id(app, "show", "打开 GitSyncTools", true, None::<&str>)
        .map_err(|error| format!("无法创建托盘菜单项：{error}"))?;
    let sync = MenuItem::with_id(app, "sync", "同步", true, None::<&str>)
        .map_err(|error| format!("无法创建托盘菜单项：{error}"))?;
    let quit = MenuItem::with_id(app, "quit", "退出 GitSyncTools", true, None::<&str>)
        .map_err(|error| format!("无法创建托盘菜单项：{error}"))?;
    let menu = Menu::with_items(app, &[&show, &sync, &quit])
        .map_err(|error| format!("无法创建托盘菜单：{error}"))?;
    let mut tray = TrayIconBuilder::with_id("main")
        .menu(&menu)
        .tooltip("GitSyncTools")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => show_main(app),
            "sync" => handle_tray_sync(app.clone()),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } | TrayIconEvent::DoubleClick {
                    button: MouseButton::Left,
                    ..
                }
            ) {
                show_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)
        .map_err(|error| format!("无法创建托盘图标：{error}"))?;
    Ok(())
}

fn notify(app: &AppHandle, title: &str, body: impl Into<String>) {
    let _ = app
        .notification()
        .builder()
        .title(title)
        .body(body.into())
        .show();
}

fn handle_tray_sync(app: AppHandle) {
    std::thread::spawn(move || match commands::sync_from_tray(&app) {
        Ok(commands::TraySyncOutcome::Completed(message)) => {
            notify(&app, "GitSyncTools", message);
        }
        Ok(commands::TraySyncOutcome::NeedsAttention(message)) => {
            notify(&app, "GitSyncTools 需要处理", message);
            show_main(&app);
        }
        Err(error) => {
            let summary: String = error.chars().take(160).collect();
            notify(&app, "GitSyncTools 同步失败", summary);
            show_main(&app);
        }
    });
}

fn handle_context_publish(app: AppHandle, paths: Vec<PathBuf>) {
    if paths.is_empty() {
        show_main(&app);
        return;
    }
    std::thread::spawn(move || {
        match commands::publish_for_context(&app, paths) {
            Ok(result) if result.pending_push => {
                let _ = app
                    .notification()
                    .builder()
                    .title("GitSyncTools")
                    .body("内容已保留，等待重新上传")
                    .show();
                show_main(&app);
            }
            Ok(result) => {
                let _ = app
                    .notification()
                    .builder()
                    .title("GitSyncTools")
                    .body(result.message)
                    .show();
            }
            Err(error) => {
                let summary: String = error.chars().take(160).collect();
                let _ = app
                    .notification()
                    .builder()
                    .title("GitSyncTools 未完成")
                    .body(summary)
                    .show();
                show_main(&app);
            }
        }
        let _ = app.emit("sync-status-updated", ());
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let paths = context_paths(&argv);
            if paths.is_empty() {
                show_main(app);
            } else {
                handle_context_publish(app.clone(), paths);
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                let shared = window.state::<AppState>();
                if !shared.tray_available.load(Ordering::SeqCst) {
                    return;
                }
                api.prevent_close();
                let _ = window.hide();
                if !shared.close_hint_shown.swap(true, Ordering::SeqCst) {
                    let _ = window
                        .app_handle()
                        .notification()
                        .builder()
                        .title("GitSyncTools 已在后台运行")
                        .body("单击系统托盘图标可重新打开，使用托盘菜单可退出。")
                        .show();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_sync_status,
            clear_last_error,
            get_repository_snapshot,
            refresh_repository,
            validate_connection,
            configure_repository,
            publish,
            delete_repository_files,
            retry_pending_push,
            prepare_pull,
            apply_pull,
            get_update_proxy,
            open_log_directory,
            record_update_event
        ])
        .setup(|app| {
            if let Err(error) = diagnostics::init(app.handle()) {
                eprintln!("failed to initialize diagnostics: {error}");
            }
            if let Err(error) = commands::clear_startup_error(app.handle()) {
                eprintln!("failed to clear stale startup error: {error}");
            }
            match create_tray(app) {
                Ok(()) => app
                    .state::<AppState>()
                    .tray_available
                    .store(true, Ordering::SeqCst),
                Err(error) => eprintln!("{error}"),
            }
            let arguments: Vec<String> = std::env::args().collect();
            let paths = context_paths(&arguments);
            if paths.is_empty() {
                show_main(app.handle());
            } else {
                handle_context_publish(app.handle().clone(), paths);
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run GitSyncTools");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_context_paths_after_flag() {
        let args = vec![
            "gitsynctools".into(),
            "--publish".into(),
            "C:\\Docs\\a.txt".into(),
        ];
        assert_eq!(context_paths(&args), vec![PathBuf::from("C:\\Docs\\a.txt")]);
    }
}
