use std::fs;
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use uuid::Uuid;

use crate::model::{AppConfig, LocalState};

#[derive(Debug, Clone)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub repository: PathBuf,
    pub config: PathBuf,
    pub state: PathBuf,
}

impl RuntimePaths {
    pub fn from_app(app: &AppHandle) -> Result<Self, String> {
        let root = app
            .path()
            .app_data_dir()
            .map_err(|error| format!("无法确定应用数据目录：{error}"))?;
        fs::create_dir_all(&root).map_err(|error| format!("无法创建应用数据目录：{error}"))?;
        Ok(Self {
            repository: root.join("repository"),
            config: root.join("config.json"),
            state: root.join("state.json"),
            root,
        })
    }
}

pub fn load_config(paths: &RuntimePaths) -> Result<Option<AppConfig>, String> {
    read_optional_json(&paths.config)
}

pub fn save_config(paths: &RuntimePaths, config: &AppConfig) -> Result<(), String> {
    write_json_atomic(&paths.config, config)
}

pub fn load_state(paths: &RuntimePaths) -> Result<LocalState, String> {
    Ok(read_optional_json(&paths.state)?.unwrap_or_default())
}

pub fn save_state(paths: &RuntimePaths, state: &LocalState) -> Result<(), String> {
    write_json_atomic(&paths.state, state)
}

pub fn read_optional_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|error| format!("{} 内容损坏：{error}", path.display()))
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 {}：{error}", parent.display()))?;
    }
    let temporary = path.with_extension(format!("tmp-{}", Uuid::new_v4()));
    let data =
        serde_json::to_vec_pretty(value).map_err(|error| format!("无法生成配置数据：{error}"))?;
    fs::write(&temporary, data)
        .map_err(|error| format!("无法写入 {}：{error}", temporary.display()))?;
    if path.exists() {
        fs::remove_file(path).map_err(|error| format!("无法替换 {}：{error}", path.display()))?;
    }
    fs::rename(&temporary, path).map_err(|error| format!("无法保存 {}：{error}", path.display()))
}
