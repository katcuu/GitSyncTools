use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};

use chrono::Utc;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use walkdir::WalkDir;

use crate::git::GitRepository;
use crate::model::{
    AppConfig, AppliedEntry, ChangeKind, ConflictAction, ConflictKind, ConflictResolution,
    DeviceRole, LocalState, Manifest, ManifestEntry, ManifestKind, PublishResult, PullChange,
    PullConflict, PullPlan, RepositoryDeleteTarget, RepositoryFileInfo, RepositoryInfo,
    RepositorySnapshot, MANIFEST_VERSION, MAX_FILE_SIZE,
};
use crate::storage::{read_optional_json, save_state, write_json_atomic, RuntimePaths};

#[derive(Debug)]
struct ValidatedSource {
    source: PathBuf,
    name: String,
}

enum ManifestLoadError {
    Missing,
    Invalid(String),
}

#[derive(Debug, PartialEq, Eq)]
struct LfsPointer {
    oid: String,
    size: u64,
}

pub fn publish_files(
    runtime: &RuntimePaths,
    config: &AppConfig,
    state: &mut LocalState,
    paths: Vec<PathBuf>,
) -> Result<PublishResult, String> {
    let _timer = crate::diagnostics::OperationTimer::new("publish_files");
    if config.role != DeviceRole::Sender {
        return Err("本机不是发送端".into());
    }
    if state.pending_push {
        return Err("上次内容尚未上传，请先重新上传".into());
    }
    let sources = validate_sources(paths)?;
    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    repository.ensure()?;
    let remote_before = repository.sync_with_remote()?;
    let mut initializing_existing_repository = false;

    let staging = runtime
        .root
        .join("staging")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&staging).map_err(|error| format!("无法创建临时目录：{error}"))?;
    let files_root = repository.root().join("files");
    fs::create_dir_all(&files_root).map_err(|error| format!("无法创建仓库文件目录：{error}"))?;
    if let Some(commit) = remote_before.as_deref() {
        match try_load_manifest_from_commit(&repository, commit) {
            Ok(remote_manifest) => {
                let replacement_roots: BTreeSet<String> = sources
                    .iter()
                    .map(|source| source.name.to_lowercase())
                    .collect();
                materialize_commit_files_with_replacements(
                    &repository,
                    commit,
                    &remote_manifest,
                    &replacement_roots,
                )?;
            }
            Err(ManifestLoadError::Missing) => initializing_existing_repository = true,
            Err(ManifestLoadError::Invalid(error)) => return Err(error),
        }
    }

    let copy_result = sources.iter().try_for_each(|source| {
        replace_selected_item(&source.source, &source.name, &files_root, &staging)
    });
    let _ = fs::remove_dir_all(&staging);
    copy_result?;

    let metadata_root = repository.root().join(".filesync");
    fs::create_dir_all(&metadata_root).map_err(|error| format!("无法创建清单目录：{error}"))?;
    fs::write(
        repository.root().join(".gitattributes"),
        "/files/** binary -filter\n",
    )
    .map_err(|error| format!("无法写入 Git 属性配置：{error}"))?;
    let manifest_path = metadata_root.join("manifest.json");
    let old_manifest: Option<Manifest> = read_optional_json(&manifest_path)?;
    let mut manifest = build_manifest(&files_root, old_manifest.as_ref())?;
    write_json_atomic(&manifest_path, &manifest)?;

    let repository_path = metadata_root.join("repository.json");
    if !repository_path.exists() {
        write_json_atomic(
            &repository_path,
            &RepositoryInfo {
                version: MANIFEST_VERSION,
                repository_id: Uuid::new_v4().to_string(),
            },
        )?;
    }

    repository.stage_all()?;
    if reconcile_manifest_with_staged(&repository, &mut manifest, old_manifest.as_ref())? {
        write_json_atomic(&manifest_path, &manifest)?;
        repository.stage_paths(&[".filesync/manifest.json".into()])?;
    }
    validate_staged_manifest(&repository, &manifest)?;
    if !repository.has_staged_changes()? {
        state.last_error = None;
        state.last_remote_commit = remote_before;
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: false,
            pending_push: false,
            commit: repository.head()?,
            message: "内容没有变化".into(),
        });
    }

    let message = format!("Sync {}", Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
    let commit = repository.commit(&message)?;
    state.pending_push = true;
    state.pending_commit = Some(commit.clone());
    state.last_error = None;
    save_state(runtime, state)?;

    if let Err(error) = repository.push_head() {
        state.last_error = Some(error.clone());
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: true,
            pending_push: true,
            commit: Some(commit),
            message: format!("内容已保留，等待重新上传：{error}"),
        });
    }

    finish_push(runtime, state, &commit)?;
    Ok(PublishResult {
        changed: true,
        pending_push: false,
        commit: Some(commit),
        message: if initializing_existing_repository {
            "同步完成；已保留仓库原有文件并创建 GitSyncTools 清单".into()
        } else {
            "同步完成".into()
        },
    })
}

pub fn delete_repository_files(
    runtime: &RuntimePaths,
    config: &AppConfig,
    state: &mut LocalState,
    entries: Vec<RepositoryDeleteTarget>,
    expected_commit: Option<&str>,
) -> Result<PublishResult, String> {
    let _timer = crate::diagnostics::OperationTimer::new("delete_repository_files");
    if state.pending_push {
        return Err("上次内容尚未上传，请先重新上传".into());
    }
    let entries = validate_delete_targets(entries)?;
    if let Some(commit) = expected_commit {
        validate_commit_id(commit)?;
    }
    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    repository.ensure()?;
    let remote_before = repository.sync_with_remote()?;
    let Some(remote_commit) = remote_before.as_deref() else {
        state.last_error = None;
        state.last_remote_commit = None;
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: false,
            pending_push: false,
            commit: None,
            message: "所选文件已在仓库中删除".into(),
        });
    };

    let manifest = match try_load_manifest_from_commit(&repository, remote_commit) {
        Ok(manifest) => {
            materialize_commit_files(&repository, remote_commit, &manifest)?;
            Some(manifest)
        }
        Err(ManifestLoadError::Missing) => None,
        Err(ManifestLoadError::Invalid(error)) => return Err(error),
    };
    let tree_objects: BTreeMap<String, String> = repository
        .list_tree_files(remote_commit)?
        .into_iter()
        .map(|file| (file.path, file.object_id))
        .collect();
    let expected_objects = match expected_commit
        .filter(|commit| *commit != remote_commit)
        .filter(|_| {
            entries.iter().any(|entry| {
                let path = if entry.managed {
                    format!("files/{}", entry.path)
                } else {
                    entry.path.clone()
                };
                tree_objects.contains_key(&path)
            })
        }) {
        Some(commit) => Some(
            repository
                .list_tree_files(commit)
                .map_err(|_| "仓库文件列表已过期，请刷新后重新选择".to_string())?
                .into_iter()
                .map(|file| (file.path, file.object_id))
                .collect::<BTreeMap<_, _>>(),
        ),
        None => None,
    };
    let managed_paths: BTreeSet<String> = manifest
        .iter()
        .flat_map(|manifest| manifest.entries.iter())
        .filter(|entry| entry.kind == ManifestKind::File)
        .map(|entry| entry.path.clone())
        .collect();
    let managed_repository_paths: BTreeSet<String> = managed_paths
        .iter()
        .map(|path| format!("files/{path}"))
        .collect();

    let mut active_entries = Vec::new();
    let mut conflicts = Vec::new();
    for entry in &entries {
        if !entry.managed && is_protected_repository_path(&entry.path) {
            return Err(format!("不允许删除受保护的仓库文件：{}", entry.path));
        }
        let repository_path = if entry.managed {
            format!("files/{}", entry.path)
        } else {
            entry.path.clone()
        };
        let Some(current_object) = tree_objects.get(&repository_path) else {
            continue;
        };
        if expected_objects
            .as_ref()
            .is_some_and(|objects| objects.get(&repository_path) != Some(current_object))
        {
            conflicts.push(entry.path.clone());
            continue;
        }
        if entry.managed {
            if !managed_paths.contains(&entry.path) {
                conflicts.push(entry.path.clone());
                continue;
            }
        } else if managed_repository_paths.contains(&entry.path) {
            conflicts.push(entry.path.clone());
            continue;
        }
        active_entries.push(entry.clone());
    }
    if !conflicts.is_empty() {
        return Err(format!(
            "以下文件在选择后已被修改，未执行删除：{}。仓库列表已更新，请确认后重试",
            conflicts.join("、")
        ));
    }
    if active_entries.is_empty() {
        state.last_error = None;
        state.last_remote_commit = remote_before.clone();
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: false,
            pending_push: false,
            commit: remote_before,
            message: "所选文件已在仓库中删除".into(),
        });
    }

    for entry in &active_entries {
        let root = if entry.managed {
            repository.root().join("files")
        } else {
            repository.root().to_path_buf()
        };
        let target = safe_join(&root, &entry.path)?;
        reject_link(&target)?;
        let metadata = target
            .metadata()
            .map_err(|error| format!("无法读取待删除文件 {}：{error}", entry.path))?;
        if !metadata.is_file() {
            return Err(format!("只能删除文件：{}", entry.path));
        }
    }
    for entry in &active_entries {
        let root = if entry.managed {
            repository.root().join("files")
        } else {
            repository.root().to_path_buf()
        };
        let target = safe_join(&root, &entry.path)?;
        fs::remove_file(&target).map_err(|error| format!("无法删除 {}：{error}", entry.path))?;
    }

    let deleted_managed: BTreeSet<&str> = active_entries
        .iter()
        .filter(|entry| entry.managed)
        .map(|entry| entry.path.as_str())
        .collect();
    if let Some(mut manifest) = manifest {
        if !deleted_managed.is_empty() {
            manifest.entries.retain(|entry| {
                entry.kind != ManifestKind::File || !deleted_managed.contains(entry.path.as_str())
            });
            write_json_atomic(
                &repository.root().join(".filesync").join("manifest.json"),
                &manifest,
            )?;
        }
        repository.stage_all()?;
    }
    let unmanaged_paths: Vec<String> = active_entries
        .iter()
        .filter(|entry| !entry.managed)
        .map(|entry| entry.path.clone())
        .collect();
    repository.stage_paths(&unmanaged_paths)?;

    if !repository.has_staged_changes()? {
        state.last_error = None;
        state.last_remote_commit = remote_before;
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: false,
            pending_push: false,
            commit: repository.head()?,
            message: "所选文件没有变化".into(),
        });
    }

    let count = active_entries.len();
    let already_deleted = entries.len() - count;
    let message = format!(
        "Delete {count} repository files {}",
        Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );
    let commit = repository.commit(&message)?;
    state.pending_push = true;
    state.pending_commit = Some(commit.clone());
    state.last_error = None;
    save_state(runtime, state)?;

    if let Err(error) = repository.push_head() {
        state.last_error = Some(error.clone());
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: true,
            pending_push: true,
            commit: Some(commit),
            message: format!("删除已保留，等待重新上传：{error}"),
        });
    }

    finish_push(runtime, state, &commit)?;
    Ok(PublishResult {
        changed: true,
        pending_push: false,
        commit: Some(commit),
        message: if already_deleted > 0 {
            format!("已删除 {count} 个仓库文件，另有 {already_deleted} 个已不存在")
        } else {
            format!("已删除 {count} 个仓库文件")
        },
    })
}

pub fn retry_pending_push(
    runtime: &RuntimePaths,
    config: &AppConfig,
    state: &mut LocalState,
) -> Result<PublishResult, String> {
    let _timer = crate::diagnostics::OperationTimer::new("retry_pending_push");
    let commit = state
        .pending_commit
        .clone()
        .filter(|_| state.pending_push)
        .ok_or_else(|| "没有等待上传的内容".to_string())?;
    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    repository.ensure()?;
    if repository.head()?.as_deref() != Some(commit.as_str()) {
        return Err("内部待上传版本不完整，请保留应用数据并检查日志".into());
    }

    let remote = repository.remote_head()?;
    if remote.as_deref() == Some(commit.as_str()) {
        finish_push(runtime, state, &commit)?;
        return Ok(PublishResult {
            changed: true,
            pending_push: false,
            commit: Some(commit),
            message: "远端已经包含该版本".into(),
        });
    }
    let parent = repository.parent_of(&commit)?;
    if remote != parent {
        return Err("远端仓库出现了本机未知的提交，已停止重新上传".into());
    }

    if let Err(error) = repository.push_head() {
        state.last_error = Some(error.clone());
        save_state(runtime, state)?;
        return Ok(PublishResult {
            changed: true,
            pending_push: true,
            commit: Some(commit),
            message: format!("仍未上传：{error}"),
        });
    }
    finish_push(runtime, state, &commit)?;
    Ok(PublishResult {
        changed: true,
        pending_push: false,
        commit: Some(commit),
        message: "重新上传成功".into(),
    })
}

pub fn refresh_repository_snapshot(
    runtime: &RuntimePaths,
    config: &AppConfig,
    state: &mut LocalState,
) -> Result<RepositorySnapshot, String> {
    let _timer = crate::diagnostics::OperationTimer::new("refresh_repository_snapshot");
    if state.pending_push {
        return Err("存在尚未上传的内容，请先重新上传".into());
    }
    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    repository.ensure()?;
    let remote = repository.sync_with_remote()?;
    state.last_remote_commit = remote;
    state.last_checked_at = Some(Utc::now().to_rfc3339());
    state.last_error = None;
    save_state(runtime, state)?;
    read_repository_snapshot(runtime, config)
}

pub fn read_repository_snapshot(
    runtime: &RuntimePaths,
    config: &AppConfig,
) -> Result<RepositorySnapshot, String> {
    if !runtime.repository.join(".git").is_dir() {
        return Ok(RepositorySnapshot::empty("内部仓库尚未初始化"));
    }
    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    let Some(commit) = repository.head()? else {
        return Ok(RepositorySnapshot::empty("仓库中暂无同步文件"));
    };
    let manifest = match try_load_manifest_from_commit(&repository, &commit) {
        Ok(manifest) => Some(manifest),
        Err(ManifestLoadError::Missing) => None,
        Err(ManifestLoadError::Invalid(error)) => return Err(error),
    };
    if let Some(manifest) = &manifest {
        validate_manifest_metadata(manifest)?;
    }

    let commit_date = repository.commit_date(&commit)?;
    let tree_files = repository.list_tree_files(&commit)?;
    let mut managed_repository_paths = BTreeSet::new();
    let mut files = Vec::new();
    let mut managed_folder_count = 0;
    if let Some(manifest) = &manifest {
        managed_folder_count = manifest
            .entries
            .iter()
            .filter(|entry| entry.kind == ManifestKind::Directory)
            .count();
        for entry in manifest
            .entries
            .iter()
            .filter(|entry| entry.kind == ManifestKind::File)
        {
            managed_repository_paths.insert(format!("files/{}", entry.path));
            files.push(RepositoryFileInfo {
                path: entry.path.clone(),
                size: entry.size,
                updated_at: entry.updated_at.clone(),
                managed: true,
            });
        }
    }

    let managed_file_count = files.len();
    let mut unmanaged_folders = BTreeSet::new();
    for tree_file in tree_files {
        if tree_file.path == ".gitattributes"
            || tree_file.path.starts_with(".filesync/")
            || managed_repository_paths.contains(&tree_file.path)
        {
            continue;
        }
        let parts: Vec<&str> = tree_file.path.split('/').collect();
        for depth in 1..parts.len() {
            unmanaged_folders.insert(parts[..depth].join("/"));
        }
        files.push(RepositoryFileInfo {
            path: tree_file.path,
            size: tree_file.size,
            updated_at: commit_date.clone(),
            managed: false,
        });
    }

    let unmanaged_file_count = files.len().saturating_sub(managed_file_count);
    let file_count = files.len();
    let folder_count = managed_folder_count + unmanaged_folders.len();
    let total_bytes = files.iter().map(|file| file.size).sum();
    files.sort_by(|left, right| {
        right
            .managed
            .cmp(&left.managed)
            .then_with(|| left.path.to_lowercase().cmp(&right.path.to_lowercase()))
    });
    let truncated = files.len() > 500;
    files.truncate(500);
    let initialized = manifest.is_some();
    let message = match (initialized, managed_file_count, unmanaged_file_count) {
        (false, _, 0) => "仓库尚未初始化，首次同步会自动创建清单".into(),
        (false, _, count) => {
            format!("仓库尚未初始化；检测到 {count} 个已有文件，首次同步会保留它们")
        }
        (true, 0, 0) => "仓库中暂无同步文件".into(),
        (true, managed, 0) => format!("仓库中共有 {managed} 个同步文件"),
        (true, managed, unmanaged) => {
            format!("仓库中有 {managed} 个同步文件，另有 {unmanaged} 个未纳入同步的仓库文件")
        }
    };
    Ok(RepositorySnapshot {
        available: true,
        commit: Some(commit),
        file_count,
        folder_count,
        managed_file_count,
        unmanaged_file_count,
        initialized,
        total_bytes,
        truncated,
        files,
        message,
    })
}

fn finish_push(runtime: &RuntimePaths, state: &mut LocalState, commit: &str) -> Result<(), String> {
    state.pending_push = false;
    state.pending_commit = None;
    state.last_error = None;
    state.last_remote_commit = Some(commit.to_owned());
    state.last_sync_at = Some(Utc::now().to_rfc3339());
    save_state(runtime, state)
}

pub fn prepare_pull_plan(
    runtime: &RuntimePaths,
    config: &AppConfig,
    state: &mut LocalState,
) -> Result<PullPlan, String> {
    let _timer = crate::diagnostics::OperationTimer::new("prepare_pull_plan");
    if config.role != DeviceRole::Receiver {
        return Err("本机不是接收端".into());
    }
    let destination = config
        .destination
        .as_ref()
        .ok_or_else(|| "尚未设置同步目录".to_string())?;
    fs::create_dir_all(destination).map_err(|error| format!("无法访问同步目录：{error}"))?;

    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    repository.ensure()?;
    let Some(remote) = repository.sync_with_remote()? else {
        state.last_error = None;
        state.last_checked_at = Some(Utc::now().to_rfc3339());
        save_state(runtime, state)?;
        return Ok(PullPlan {
            repository_empty: true,
            commit: None,
            changes: vec![],
            conflicts: vec![],
            message: "仓库暂无文件".into(),
        });
    };
    let manifest = match try_load_manifest_from_commit(&repository, &remote) {
        Ok(manifest) => manifest,
        Err(ManifestLoadError::Missing) => {
            state.last_error = None;
            state.last_checked_at = Some(Utc::now().to_rfc3339());
            state.last_remote_commit = Some(remote.clone());
            save_state(runtime, state)?;
            return Ok(PullPlan {
                repository_empty: true,
                commit: Some(remote),
                changes: vec![],
                conflicts: vec![],
                message: "仓库尚未由 GitSyncTools 初始化，请先在发送端同步文件".into(),
            });
        }
        Err(ManifestLoadError::Invalid(error)) => return Err(error),
    };
    materialize_commit_files(&repository, &remote, &manifest)?;

    let plan = compare_destination(destination, state, &manifest, &remote)?;
    state.last_error = None;
    state.last_checked_at = Some(Utc::now().to_rfc3339());
    state.last_remote_commit = Some(remote);
    save_state(runtime, state)?;
    Ok(plan)
}

pub fn apply_pull_plan(
    runtime: &RuntimePaths,
    config: &AppConfig,
    state: &mut LocalState,
    expected_commit: &str,
    resolutions: &[ConflictResolution],
) -> Result<(), String> {
    let _timer = crate::diagnostics::OperationTimer::new("apply_pull_plan");
    let plan = prepare_pull_plan(runtime, config, state)?;
    if plan.commit.as_deref() != Some(expected_commit) {
        return Err("远端在确认期间发生变化，请重新检查更新".into());
    }
    let resolution_map: HashMap<&str, &ConflictAction> = resolutions
        .iter()
        .map(|resolution| (resolution.path.as_str(), &resolution.action))
        .collect();
    for conflict in &plan.conflicts {
        if !resolution_map.contains_key(conflict.path.as_str()) {
            return Err(format!("尚未选择 {} 的处理方式", conflict.path));
        }
    }

    let destination = config
        .destination
        .as_ref()
        .ok_or_else(|| "尚未设置同步目录".to_string())?;
    let repository = GitRepository::new(runtime.repository.clone(), config)?;
    let manifest = load_manifest_from_commit(&repository, expected_commit)?;
    materialize_commit_files(&repository, expected_commit, &manifest)?;
    let desired: BTreeMap<String, ManifestEntry> = manifest
        .entries
        .iter()
        .cloned()
        .map(|entry| (entry.path.clone(), entry))
        .collect();

    let backup_root = destination
        .join(".gitsynctools-backups")
        .join(Utc::now().format("%Y%m%d-%H%M%S").to_string());
    let mut next_applied = state.applied_entries.clone();
    let mut kept_roots: Vec<String> = Vec::new();

    let mut removed: Vec<String> = state
        .applied_entries
        .keys()
        .filter(|path| !desired.contains_key(*path))
        .cloned()
        .collect();
    removed.sort_by_key(|path| std::cmp::Reverse(path_depth(path)));
    for path in &removed {
        if is_below_any(path, &kept_roots) {
            continue;
        }
        let target = safe_join(destination, path)?;
        match resolution_map.get(path.as_str()).copied() {
            Some(ConflictAction::Keep) => {
                kept_roots.push(path.clone());
                continue;
            }
            Some(ConflictAction::Backup) => {
                backup_existing(&target, &safe_join(&backup_root, path)?)?
            }
            Some(ConflictAction::Overwrite) | None => remove_managed_target(&target, false)?,
        }
        next_applied.remove(path);
    }

    let mut entries = manifest.entries.clone();
    entries.sort_by_key(|entry| {
        (
            entry.kind != ManifestKind::Directory,
            path_depth(&entry.path),
        )
    });
    for entry in &entries {
        if is_below_any(&entry.path, &kept_roots) {
            next_applied.remove(&entry.path);
            continue;
        }
        let target = safe_join(destination, &entry.path)?;
        match resolution_map.get(entry.path.as_str()).copied() {
            Some(ConflictAction::Keep) => {
                kept_roots.push(entry.path.clone());
                continue;
            }
            Some(ConflictAction::Backup) => {
                backup_existing(&target, &safe_join(&backup_root, &entry.path)?)?
            }
            Some(ConflictAction::Overwrite) => remove_managed_target(&target, true)?,
            None => {}
        }

        match entry.kind {
            ManifestKind::Directory => {
                if target.exists() && !target.is_dir() {
                    remove_managed_target(&target, true)?;
                }
                fs::create_dir_all(&target)
                    .map_err(|error| format!("无法创建 {}：{error}", target.display()))?;
            }
            ManifestKind::File => {
                let source = safe_join(&repository.root().join("files"), &entry.path)?;
                if target.is_dir() {
                    remove_managed_target(&target, true)?;
                }
                atomic_copy_file(&source, &target)?;
            }
        }
        next_applied.insert(
            entry.path.clone(),
            AppliedEntry {
                kind: entry.kind.clone(),
                sha256: entry.sha256.clone(),
            },
        );
    }

    state.applied_entries = next_applied;
    state.last_applied_commit = Some(expected_commit.to_owned());
    state.last_remote_commit = Some(expected_commit.to_owned());
    state.last_sync_at = Some(Utc::now().to_rfc3339());
    state.last_checked_at = state.last_sync_at.clone();
    state.last_error = None;
    save_state(runtime, state)
}

fn compare_destination(
    destination: &Path,
    state: &LocalState,
    manifest: &Manifest,
    commit: &str,
) -> Result<PullPlan, String> {
    let desired: BTreeMap<&str, &ManifestEntry> = manifest
        .entries
        .iter()
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let mut changes = Vec::new();
    let mut conflicts = Vec::new();

    for entry in &manifest.entries {
        let previous = state.applied_entries.get(&entry.path);
        let target = safe_join(destination, &entry.path)?;
        let local_conflict = match previous {
            Some(applied) if applied.kind != entry.kind => Some(ConflictKind::TypeChanged),
            Some(applied) => local_difference(&target, applied)?,
            None => unmanaged_difference(&target, &entry.kind),
        };
        let remote_changed = previous
            .map(|applied| applied.kind != entry.kind || applied.sha256 != entry.sha256)
            .unwrap_or(true);

        if remote_changed || local_conflict.is_some() {
            changes.push(PullChange {
                path: entry.path.clone(),
                kind: if previous.is_some() {
                    ChangeKind::Update
                } else {
                    ChangeKind::Add
                },
            });
        }
        if let Some(kind) = local_conflict {
            conflicts.push(PullConflict {
                path: entry.path.clone(),
                kind,
                remote_deleted: false,
            });
        }
    }

    for (path, applied) in &state.applied_entries {
        if desired.contains_key(path.as_str()) {
            continue;
        }
        changes.push(PullChange {
            path: path.clone(),
            kind: ChangeKind::Delete,
        });
        let target = safe_join(destination, path)?;
        if target.exists() && local_difference(&target, applied)?.is_some() {
            conflicts.push(PullConflict {
                path: path.clone(),
                kind: ConflictKind::RemoteDeletedLocalModified,
                remote_deleted: true,
            });
        }
    }

    changes.sort_by(|left, right| left.path.cmp(&right.path));
    conflicts.sort_by(|left, right| left.path.cmp(&right.path));
    conflicts.dedup_by(|left, right| left.path == right.path);
    let message = if changes.is_empty() {
        "已经是最新版本".into()
    } else {
        format!("发现 {} 项变化", changes.len())
    };
    Ok(PullPlan {
        repository_empty: false,
        commit: Some(commit.to_owned()),
        changes,
        conflicts,
        message,
    })
}

fn local_difference(target: &Path, applied: &AppliedEntry) -> Result<Option<ConflictKind>, String> {
    if !target.exists() {
        return Ok(Some(ConflictKind::LocalDeleted));
    }
    match applied.kind {
        ManifestKind::Directory if !target.is_dir() => Ok(Some(ConflictKind::TypeChanged)),
        ManifestKind::Directory => Ok(None),
        ManifestKind::File if !target.is_file() => Ok(Some(ConflictKind::TypeChanged)),
        ManifestKind::File => {
            let hash = hash_file(target)?;
            if Some(hash) == applied.sha256 {
                Ok(None)
            } else {
                Ok(Some(ConflictKind::LocalModified))
            }
        }
    }
}

fn unmanaged_difference(target: &Path, desired: &ManifestKind) -> Option<ConflictKind> {
    if !target.exists() {
        return None;
    }
    match desired {
        ManifestKind::Directory if target.is_dir() => None,
        ManifestKind::Directory => Some(ConflictKind::TypeChanged),
        ManifestKind::File => Some(ConflictKind::UnmanagedCollision),
    }
}

fn validate_delete_targets(
    entries: Vec<RepositoryDeleteTarget>,
) -> Result<Vec<RepositoryDeleteTarget>, String> {
    if entries.is_empty() {
        return Err("请选择要删除的文件".into());
    }
    if entries.len() > 500 {
        return Err("一次最多删除 500 个文件".into());
    }
    let mut unique = BTreeSet::new();
    for entry in &entries {
        validate_manifest_path(&entry.path)?;
        let key = (entry.managed, entry.path.to_lowercase());
        if !unique.insert(key) {
            return Err(format!("删除列表包含重复文件：{}", entry.path));
        }
    }
    Ok(entries)
}

fn validate_commit_id(commit: &str) -> Result<(), String> {
    if matches!(commit.len(), 40 | 64) && commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("仓库文件列表版本无效，请刷新后重试".into())
    }
}

fn is_protected_repository_path(path: &str) -> bool {
    path == ".gitattributes" || path == ".filesync" || path.starts_with(".filesync/")
}

fn validate_sources(paths: Vec<PathBuf>) -> Result<Vec<ValidatedSource>, String> {
    if paths.is_empty() {
        return Err("请选择文件或文件夹".into());
    }
    let mut names = BTreeSet::new();
    let mut validated = Vec::with_capacity(paths.len());
    for path in paths {
        reject_link(&path)?;
        let canonical = dunce::canonicalize(&path)
            .map_err(|error| format!("无法访问 {}：{error}", path.display()))?;
        let name = canonical
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("{} 不能作为顶层同步项", path.display()))?
            .to_owned();
        let normalized = name.to_lowercase();
        if !names.insert(normalized) {
            return Err(format!("同一次选择中存在重复名称：{name}"));
        }
        validate_source_tree(&canonical)?;
        validated.push(ValidatedSource {
            source: canonical,
            name,
        });
    }
    Ok(validated)
}

fn validate_source_tree(root: &Path) -> Result<(), String> {
    for entry in WalkDir::new(root).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|error| format!("无法读取选择内容：{error}"))?;
        reject_link(entry.path())?;
        let metadata = entry
            .metadata()
            .map_err(|error| format!("无法读取 {}：{error}", entry.path().display()))?;
        if metadata.is_file() && metadata.len() > MAX_FILE_SIZE {
            return Err(format!("{} 超过 50MB 限制", entry.path().display()));
        }
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(format!("不支持的文件类型：{}", entry.path().display()));
        }
    }
    Ok(())
}

fn reject_link(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata) {
        Err(format!("不支持符号链接或联接点：{}", path.display()))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
fn is_windows_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    metadata.file_attributes() & 0x400 != 0
}

#[cfg(not(windows))]
fn is_windows_reparse_point(_: &fs::Metadata) -> bool {
    false
}

fn replace_selected_item(
    source: &Path,
    name: &str,
    files_root: &Path,
    staging_root: &Path,
) -> Result<(), String> {
    let staged = staging_root.join(name);
    copy_tree(source, &staged)?;
    let target = files_root.join(name);
    remove_managed_target(&target, true)?;
    fs::rename(&staged, &target).map_err(|error| format!("无法替换仓库中的 {name}：{error}"))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    if source.is_file() {
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("无法创建临时目录：{error}"))?;
        }
        fs::copy(source, destination)
            .map_err(|error| format!("无法复制 {}：{error}", source.display()))?;
        return Ok(());
    }
    for entry in WalkDir::new(source).follow_links(false).sort_by_file_name() {
        let entry = entry.map_err(|error| format!("无法复制目录：{error}"))?;
        let relative = entry
            .path()
            .strip_prefix(source)
            .map_err(|_| "无法计算文件相对路径".to_string())?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| format!("无法创建 {}：{error}", target.display()))?;
        } else {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("无法创建 {}：{error}", parent.display()))?;
            }
            fs::copy(entry.path(), &target)
                .map_err(|error| format!("无法复制 {}：{error}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn build_manifest(files_root: &Path, old: Option<&Manifest>) -> Result<Manifest, String> {
    let old_entries: BTreeMap<&str, &ManifestEntry> = old
        .into_iter()
        .flat_map(|manifest| manifest.entries.iter())
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let now = Utc::now().to_rfc3339();
    let mut entries = Vec::new();
    if files_root.exists() {
        for entry in WalkDir::new(files_root)
            .min_depth(1)
            .follow_links(false)
            .sort_by_file_name()
        {
            let entry = entry.map_err(|error| format!("无法生成文件清单：{error}"))?;
            reject_link(entry.path())?;
            let relative = entry
                .path()
                .strip_prefix(files_root)
                .map_err(|_| "无法生成相对路径".to_string())?;
            let path = manifest_path(relative)?;
            let metadata = entry
                .metadata()
                .map_err(|error| format!("无法读取 {}：{error}", entry.path().display()))?;
            let (kind, size, sha256) = if metadata.is_dir() {
                (ManifestKind::Directory, 0, None)
            } else {
                if metadata.len() > MAX_FILE_SIZE {
                    return Err(format!("{} 超过 50MB 限制", entry.path().display()));
                }
                (
                    ManifestKind::File,
                    metadata.len(),
                    Some(hash_file(entry.path())?),
                )
            };
            let updated_at = old_entries
                .get(path.as_str())
                .filter(|old| old.kind == kind && old.size == size && old.sha256 == sha256)
                .map(|old| old.updated_at.clone())
                .unwrap_or_else(|| now.clone());
            entries.push(ManifestEntry {
                path,
                kind,
                size,
                sha256,
                updated_at,
            });
        }
    }
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Manifest {
        version: MANIFEST_VERSION,
        entries,
    })
}

fn try_load_manifest_from_commit(
    repository: &GitRepository,
    commit: &str,
) -> Result<Manifest, ManifestLoadError> {
    let bytes = repository
        .read_blob(commit, ".filesync/manifest.json")
        .map_err(|_| ManifestLoadError::Missing)?;
    if bytes.len() > 10 * 1024 * 1024 {
        return Err(ManifestLoadError::Invalid(
            "仓库清单超过 10MB 安全限制".into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|error| ManifestLoadError::Invalid(format!("仓库清单内容损坏：{error}")))
}

fn load_manifest_from_commit(repository: &GitRepository, commit: &str) -> Result<Manifest, String> {
    try_load_manifest_from_commit(repository, commit).map_err(|error| match error {
        ManifestLoadError::Missing => "仓库中没有有效的 GitSyncTools 清单，已停止更新".into(),
        ManifestLoadError::Invalid(error) => error,
    })
}

fn materialize_commit_files(
    repository: &GitRepository,
    commit: &str,
    manifest: &Manifest,
) -> Result<(), String> {
    materialize_commit_files_with_replacements(repository, commit, manifest, &BTreeSet::new())
}

fn materialize_commit_files_with_replacements(
    repository: &GitRepository,
    commit: &str,
    manifest: &Manifest,
    replacement_roots: &BTreeSet<String>,
) -> Result<(), String> {
    validate_manifest_metadata(manifest)?;
    let files_root = repository.root().join("files");
    if validate_manifest(manifest, &files_root).is_ok() {
        for entry in manifest
            .entries
            .iter()
            .filter(|entry| entry.kind == ManifestKind::Directory)
        {
            let target = safe_join(&files_root, &entry.path)?;
            fs::create_dir_all(&target).map_err(|error| format!("无法重建内部目录：{error}"))?;
        }
        log::info!(
            "operation=materialize_commit_files mode=verified_worktree files={}",
            manifest
                .entries
                .iter()
                .filter(|entry| entry.kind == ManifestKind::File)
                .count()
        );
        return Ok(());
    }
    let materialized_root = repository
        .root()
        .join(format!(".gitsynctools-materialize-{}", Uuid::new_v4()));
    fs::create_dir_all(&materialized_root)
        .map_err(|error| format!("无法创建内部文件临时目录：{error}"))?;

    let mut directories: Vec<&ManifestEntry> = manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == ManifestKind::Directory)
        .collect();
    directories.sort_by_key(|entry| path_depth(&entry.path));
    for entry in directories {
        let target = safe_join(&materialized_root, &entry.path)?;
        fs::create_dir_all(&target).map_err(|error| format!("无法重建内部目录：{error}"))?;
    }

    let materialize_result = manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == ManifestKind::File)
        .try_for_each(|entry| -> Result<(), String> {
            let object_path = format!("files/{}", entry.path);
            let object_bytes = repository.read_blob(commit, &object_path)?;
            let object_hash = hash_bytes(&object_bytes);
            let bytes = if object_bytes.len() as u64 == entry.size
                && Some(object_hash.as_str()) == entry.sha256.as_deref()
            {
                object_bytes
            } else if replacement_roots.iter().any(|root| {
                let path = entry.path.to_lowercase();
                path == *root || path.starts_with(&format!("{root}/"))
            }) {
                log::warn!(
                    "operation=manifest_mismatch path={} recovery=selected_source_replacement object_size={} expected_size={}",
                    entry.path,
                    object_bytes.len(),
                    entry.size
                );
                object_bytes
            } else {
                let checked_out = safe_join(&files_root, &entry.path)?;
                let checked_out_bytes = if checked_out.exists() {
                    reject_link(&checked_out)?;
                    fs::read(&checked_out).ok()
                } else {
                    None
                };
                let checked_out_hash = checked_out_bytes.as_deref().map(hash_bytes);
                let checkout_valid = checked_out_bytes.as_ref().is_some_and(|bytes| {
                    bytes.len() as u64 == entry.size
                        && checked_out_hash.as_deref() == entry.sha256.as_deref()
                });
                let lfs_pointer = parse_lfs_pointer(&object_bytes);
                log::warn!(
                    "operation=manifest_mismatch path={} expected_size={} expected_sha256={} object_size={} object_sha256={} object_kind={} checkout_size={} checkout_sha256={} checkout_valid={}",
                    entry.path,
                    entry.size,
                    entry.sha256.as_deref().unwrap_or("missing"),
                    object_bytes.len(),
                    object_hash,
                    if lfs_pointer.is_some() { "lfs_pointer" } else { "blob" },
                    checked_out_bytes.as_ref().map(Vec::len).unwrap_or(0),
                    checked_out_hash.as_deref().unwrap_or("missing"),
                    checkout_valid
                );
                if checkout_valid {
                    checked_out_bytes.unwrap_or_default()
                } else if let Some(pointer) = lfs_pointer.filter(|pointer| {
                    pointer.size == entry.size
                        && Some(pointer.oid.as_str()) == entry.sha256.as_deref()
                }) {
                    match repository.smudge_lfs_pointer(&object_bytes) {
                        Ok(recovered)
                            if recovered.len() as u64 == pointer.size
                                && hash_bytes(&recovered) == pointer.oid =>
                        {
                            log::info!(
                                "operation=manifest_mismatch path={} recovery=git_lfs_success",
                                entry.path
                            );
                            recovered
                        }
                        Ok(recovered) => {
                            log::warn!(
                                "operation=manifest_mismatch path={} recovery=git_lfs_invalid recovered_size={} recovered_sha256={}",
                                entry.path,
                                recovered.len(),
                                hash_bytes(&recovered)
                            );
                            return Err(format!(
                                "Git LFS 未能还原文件：{}。请在 Windows 发送端使用 GitSyncTools v0.3.8 或更高版本重新同步该文件",
                                entry.path
                            ));
                        }
                        Err(error) => {
                            log::warn!(
                                "operation=manifest_mismatch path={} recovery=git_lfs_failed detail={}",
                                entry.path,
                                crate::diagnostics::safe_detail(&error)
                            );
                            return Err(format!(
                                "远端文件是 Git LFS 指针，但 macOS 无法还原：{}。请安装 Git LFS 后重试，或在 Windows 发送端使用 GitSyncTools v0.3.8 或更高版本重新同步该文件",
                                entry.path
                            ));
                        }
                    }
                } else {
                    return Err(format!(
                        "Git 对象与清单不一致：{}。远端旧提交中没有可校验的原文件，请在 Windows 发送端重新同步该文件",
                        entry.path
                    ));
                }
            };
            let target = safe_join(&materialized_root, &entry.path)?;
            atomic_write_bytes(&bytes, &target)?;
            Ok(())
        });
    if let Err(error) = materialize_result {
        let _ = fs::remove_dir_all(&materialized_root);
        return Err(error);
    }

    remove_managed_target(&files_root, true)?;
    fs::rename(&materialized_root, &files_root)
        .map_err(|error| format!("无法替换内部文件目录：{error}"))?;
    Ok(())
}

fn validate_manifest(manifest: &Manifest, files_root: &Path) -> Result<(), String> {
    validate_manifest_metadata(manifest)?;
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == ManifestKind::File)
    {
        let source = safe_join(files_root, &entry.path)?;
        reject_link(&source)?;
        let metadata = source
            .metadata()
            .map_err(|error| format!("仓库文件缺失 {}：{error}", entry.path))?;
        let actual_hash = hash_file(&source)?;
        if !metadata.is_file()
            || metadata.len() != entry.size
            || Some(actual_hash.as_str()) != entry.sha256.as_deref()
        {
            return Err(format!(
                "仓库文件与清单不一致：{}（大小 {} / {}，SHA-256 {} / {}）",
                entry.path,
                metadata.len(),
                entry.size,
                actual_hash,
                entry.sha256.as_deref().unwrap_or("missing")
            ));
        }
    }
    Ok(())
}

fn reconcile_manifest_with_staged(
    repository: &GitRepository,
    manifest: &mut Manifest,
    old_manifest: Option<&Manifest>,
) -> Result<bool, String> {
    validate_manifest_metadata(manifest)?;
    let old_entries: BTreeMap<&str, &ManifestEntry> = old_manifest
        .into_iter()
        .flat_map(|manifest| manifest.entries.iter())
        .map(|entry| (entry.path.as_str(), entry))
        .collect();
    let now = Utc::now().to_rfc3339();
    let mut changed = false;
    for entry in manifest
        .entries
        .iter_mut()
        .filter(|entry| entry.kind == ManifestKind::File)
    {
        let repository_path = format!("files/{}", entry.path);
        let staged = repository.read_staged_blob(&repository_path)?;
        let staged_hash = hash_bytes(&staged);
        let staged_size = staged.len() as u64;
        if staged_size == entry.size && Some(staged_hash.as_str()) == entry.sha256.as_deref() {
            continue;
        }
        log::info!(
            "operation=staged_content_transformed path={} worktree_size={} worktree_sha256={} staged_size={} staged_sha256={} canonical=git_index",
            entry.path,
            entry.size,
            entry.sha256.as_deref().unwrap_or("missing"),
            staged_size,
            staged_hash
        );
        entry.size = staged_size;
        entry.sha256 = Some(staged_hash.clone());
        entry.updated_at = old_entries
            .get(entry.path.as_str())
            .filter(|old| {
                old.kind == ManifestKind::File
                    && old.size == staged_size
                    && old.sha256.as_deref() == Some(staged_hash.as_str())
            })
            .map(|old| old.updated_at.clone())
            .unwrap_or_else(|| now.clone());
        changed = true;
    }
    Ok(changed)
}

fn validate_staged_manifest(repository: &GitRepository, manifest: &Manifest) -> Result<(), String> {
    validate_manifest_metadata(manifest)?;
    for entry in manifest
        .entries
        .iter()
        .filter(|entry| entry.kind == ManifestKind::File)
    {
        let repository_path = format!("files/{}", entry.path);
        let staged = repository.read_staged_blob(&repository_path)?;
        let staged_hash = hash_bytes(&staged);
        if staged.len() as u64 != entry.size
            || Some(staged_hash.as_str()) != entry.sha256.as_deref()
        {
            return Err(format!("Git 暂存对象与同步清单不一致：{}", entry.path));
        }
    }
    Ok(())
}

fn parse_lfs_pointer(bytes: &[u8]) -> Option<LfsPointer> {
    if bytes.len() > 1024 {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    if !text
        .lines()
        .any(|line| line.trim() == "version https://git-lfs.github.com/spec/v1")
    {
        return None;
    }
    let oid = text.lines().find_map(|line| {
        line.trim()
            .strip_prefix("oid sha256:")
            .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .map(str::to_owned)
    })?;
    let size = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("size "))?
        .parse()
        .ok()?;
    Some(LfsPointer { oid, size })
}

fn validate_manifest_metadata(manifest: &Manifest) -> Result<(), String> {
    if manifest.version != MANIFEST_VERSION {
        return Err(format!("不支持清单版本 {}", manifest.version));
    }
    let mut normalized = BTreeSet::new();
    let mut file_paths = BTreeSet::new();
    for entry in &manifest.entries {
        validate_manifest_path(&entry.path)?;
        if !normalized.insert(entry.path.to_lowercase()) {
            return Err(format!("清单包含重复路径：{}", entry.path));
        }
        match entry.kind {
            ManifestKind::Directory => {
                if entry.sha256.is_some() || entry.size != 0 {
                    return Err(format!("目录清单数据无效：{}", entry.path));
                }
            }
            ManifestKind::File => {
                let valid_hash = entry.sha256.as_deref().is_some_and(|hash| {
                    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
                });
                if entry.size > MAX_FILE_SIZE || !valid_hash {
                    return Err(format!("文件清单数据无效：{}", entry.path));
                }
                file_paths.insert(entry.path.clone());
            }
        }
    }
    for entry in &manifest.entries {
        let mut prefix = String::new();
        let mut parts = entry.path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if file_paths.contains(&prefix) {
                return Err(format!("清单路径位于文件内部：{}", entry.path));
            }
        }
    }
    Ok(())
}

fn validate_manifest_path(value: &str) -> Result<(), String> {
    if value.is_empty() || value.contains('\\') || value.starts_with('/') || value.ends_with('/') {
        return Err(format!("清单路径无效：{value}"));
    }
    let path = Path::new(value);
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("清单路径越界：{value}"));
    }
    Ok(())
}

fn safe_join(root: &Path, relative: &str) -> Result<PathBuf, String> {
    validate_manifest_path(relative)?;
    let mut result = root.to_path_buf();
    reject_existing_link(&result)?;
    for part in relative.split('/') {
        result.push(part);
        reject_existing_link(&result)?;
    }
    Ok(result)
}

fn reject_existing_link(path: &Path) -> Result<(), String> {
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.file_type().is_symlink() || is_windows_reparse_point(&metadata) {
            return Err(format!("路径中包含符号链接或联接点：{}", path.display()));
        }
    }
    Ok(())
}

fn manifest_path(path: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(value) => parts.push(
                value
                    .to_str()
                    .ok_or_else(|| format!("文件名不是有效 Unicode：{}", path.display()))?,
            ),
            _ => return Err(format!("文件路径无效：{}", path.display())),
        }
    }
    Ok(parts.join("/"))
}

fn hash_file(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| format!("无法读取 {}：{error}", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn atomic_write_bytes(bytes: &[u8], destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 {}：{error}", parent.display()))?;
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let temporary = destination.with_file_name(format!(".{name}.gitsynctools-{}", Uuid::new_v4()));
    fs::write(&temporary, bytes)
        .map_err(|error| format!("无法写入 {}：{error}", temporary.display()))?;
    replace_temporary_file(&temporary, destination)
}

fn atomic_copy_file(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("无法创建 {}：{error}", parent.display()))?;
    }
    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let temporary = destination.with_file_name(format!(".{name}.gitsynctools-{}", Uuid::new_v4()));
    fs::copy(source, &temporary)
        .map_err(|error| format!("无法复制 {}：{error}", source.display()))?;
    replace_temporary_file(&temporary, destination)
}

fn replace_temporary_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    if !destination.exists() {
        return fs::rename(temporary, destination)
            .map_err(|error| format!("无法写入 {}：{error}", destination.display()));
    }
    if destination.is_dir() {
        let _ = fs::remove_file(temporary);
        return Err(format!("目标路径是目录：{}", destination.display()));
    }

    let name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("file");
    let rollback =
        destination.with_file_name(format!(".{name}.gitsynctools-old-{}", Uuid::new_v4()));
    fs::rename(destination, &rollback)
        .map_err(|error| format!("无法准备替换 {}：{error}", destination.display()))?;
    if let Err(error) = fs::rename(temporary, destination) {
        let restore_error = fs::rename(&rollback, destination).err();
        let _ = fs::remove_file(temporary);
        return Err(match restore_error {
            Some(restore) => format!(
                "无法写入且无法恢复 {}：{error}；{restore}",
                destination.display()
            ),
            None => format!("无法写入 {}，原文件已恢复：{error}", destination.display()),
        });
    }
    fs::remove_file(&rollback).map_err(|error| format!("新文件已写入，但无法清理旧副本：{error}"))
}

fn backup_existing(source: &Path, backup: &Path) -> Result<(), String> {
    if !source.exists() {
        return Ok(());
    }
    if let Some(parent) = backup.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("无法创建备份目录：{error}"))?;
    }
    fs::rename(source, backup).map_err(|error| format!("无法备份 {}：{error}", source.display()))
}

fn remove_managed_target(path: &Path, recursive: bool) -> Result<(), String> {
    if !path.exists() {
        return Ok(());
    }
    if path.is_dir() {
        if recursive {
            fs::remove_dir_all(path)
                .map_err(|error| format!("无法删除 {}：{error}", path.display()))
        } else {
            match fs::remove_dir(path) {
                Ok(()) => Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => Ok(()),
                Err(error) => Err(format!("无法删除 {}：{error}", path.display())),
            }
        }
    } else {
        fs::remove_file(path).map_err(|error| format!("无法删除 {}：{error}", path.display()))
    }
}

fn path_depth(path: &str) -> usize {
    path.split('/').count()
}

fn is_below_any(path: &str, roots: &[String]) -> bool {
    roots
        .iter()
        .any(|root| path == root || path.starts_with(&format!("{root}/")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use tempfile::tempdir;

    fn runtime(root: &Path) -> RuntimePaths {
        RuntimePaths {
            root: root.to_path_buf(),
            repository: root.join("repository"),
            config: root.join("config.json"),
            state: root.join("state.json"),
        }
    }

    #[test]
    fn rejects_path_traversal() {
        assert!(validate_manifest_path("folder/file.txt").is_ok());
        assert!(validate_manifest_path("../secret.txt").is_err());
        assert!(validate_manifest_path("folder\\file.txt").is_err());
        assert!(validate_manifest_path("/absolute.txt").is_err());
        assert!(validate_delete_targets(vec![RepositoryDeleteTarget {
            path: "../secret.txt".into(),
            managed: false,
        }])
        .is_err());
        assert!(is_protected_repository_path(".filesync/manifest.json"));
        assert!(is_protected_repository_path(".gitattributes"));
        assert!(!is_protected_repository_path("README.md"));
        assert!(validate_commit_id("0123456789012345678901234567890123456789").is_ok());
        assert!(validate_commit_id("HEAD").is_err());
    }

    #[test]
    fn manifest_includes_empty_directories_and_hashes() {
        let root = tempdir().unwrap();
        fs::create_dir_all(root.path().join("empty")).unwrap();
        fs::write(root.path().join("hello.txt"), b"hello").unwrap();
        let manifest = build_manifest(root.path(), None).unwrap();
        assert_eq!(manifest.entries.len(), 2);
        assert_eq!(manifest.entries[0].kind, ManifestKind::Directory);
        assert_eq!(
            manifest.entries[1].sha256.as_deref(),
            Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824")
        );
    }

    #[test]
    fn parses_standard_git_lfs_pointer() {
        let oid = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let pointer =
            format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 5\n");
        assert_eq!(
            parse_lfs_pointer(pointer.as_bytes()),
            Some(LfsPointer {
                oid: oid.into(),
                size: 5,
            })
        );
        assert!(parse_lfs_pointer(b"ordinary file").is_none());
    }

    #[test]
    fn detects_modified_local_file() {
        let root = tempdir().unwrap();
        let path = root.path().join("note.txt");
        fs::write(&path, b"changed").unwrap();
        let applied = AppliedEntry {
            kind: ManifestKind::File,
            sha256: Some("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into()),
        };
        assert_eq!(
            local_difference(&path, &applied).unwrap(),
            Some(ConflictKind::LocalModified)
        );
    }

    #[test]
    fn publish_manifest_tracks_git_processed_content_for_all_file_types() {
        let temp = tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        let init = Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap();
        assert!(init.status.success());

        let runtime = runtime(&temp.path().join("sender-data"));
        fs::create_dir_all(&runtime.root).unwrap();
        let config = AppConfig {
            repository_url: remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            role: DeviceRole::Sender,
            destination: None,
        };
        let repository = GitRepository::new(runtime.repository.clone(), &config).unwrap();
        repository.ensure().unwrap();

        fs::write(
            runtime.repository.join(".git/info/attributes"),
            "*.docx filter=mutate\n*.bin filter=mutate\n",
        )
        .unwrap();
        for args in [
            vec![
                "config".to_string(),
                "filter.mutate.clean".into(),
                "git hash-object --stdin".into(),
            ],
            vec!["config".into(), "filter.mutate.smudge".into(), "cat".into()],
        ] {
            let output = Command::new("git")
                .args(args)
                .current_dir(&runtime.repository)
                .output()
                .unwrap();
            assert!(output.status.success());
        }

        let source = temp.path().join("selected");
        fs::create_dir(&source).unwrap();
        let document = b"PK\x03\x04office document bytes\r\n";
        let binary = b"arbitrary binary bytes\0\x01\x02";
        fs::write(source.join("document.docx"), document).unwrap();
        fs::write(source.join("payload.bin"), binary).unwrap();
        let result =
            publish_files(&runtime, &config, &mut LocalState::default(), vec![source]).unwrap();
        let commit = result.commit.as_deref().unwrap();
        let committed_document = repository
            .read_blob(commit, "files/selected/document.docx")
            .unwrap();
        let committed_binary = repository
            .read_blob(commit, "files/selected/payload.bin")
            .unwrap();
        assert_ne!(committed_document, document);
        assert_ne!(committed_binary, binary);

        let manifest: Manifest = serde_json::from_slice(
            &repository
                .read_blob(commit, ".filesync/manifest.json")
                .unwrap(),
        )
        .unwrap();
        for (path, contents) in [
            ("selected/document.docx", committed_document),
            ("selected/payload.bin", committed_binary),
        ] {
            let entry = manifest
                .entries
                .iter()
                .find(|entry| entry.path == path)
                .unwrap();
            assert_eq!(entry.size, contents.len() as u64);
            assert_eq!(
                entry.sha256.as_deref(),
                Some(hash_bytes(&contents).as_str())
            );
        }
    }

    #[test]
    fn materialize_uses_verified_smudged_file_for_legacy_filtered_commit() {
        let temp = tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        let init = Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap();
        assert!(init.status.success());

        let runtime = runtime(&temp.path().join("receiver-data"));
        fs::create_dir_all(&runtime.root).unwrap();
        let config = AppConfig {
            repository_url: remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            role: DeviceRole::Receiver,
            destination: Some(temp.path().join("destination")),
        };
        let repository = GitRepository::new(runtime.repository.clone(), &config).unwrap();
        repository.ensure().unwrap();

        let original = b"PK\x03\x04original office document";
        let desired = temp.path().join("desired");
        fs::create_dir_all(&desired).unwrap();
        fs::write(desired.join("document.docx"), original).unwrap();
        let manifest = build_manifest(&desired, None).unwrap();

        let files_root = runtime.repository.join("files");
        fs::create_dir_all(&files_root).unwrap();
        fs::write(
            files_root.join("document.docx"),
            b"version https://git-lfs.github.com/spec/v1\n",
        )
        .unwrap();
        let metadata_root = runtime.repository.join(".filesync");
        fs::create_dir_all(&metadata_root).unwrap();
        write_json_atomic(&metadata_root.join("manifest.json"), &manifest).unwrap();
        repository.stage_all().unwrap();
        let commit = repository.commit("Legacy filtered commit").unwrap();

        fs::write(files_root.join("document.docx"), original).unwrap();
        materialize_commit_files(&repository, &commit, &manifest).unwrap();
        assert_eq!(
            fs::read(files_root.join("document.docx")).unwrap(),
            original
        );
    }

    #[test]
    fn publish_repairs_corrupt_selected_file_from_legacy_commit() {
        let temp = tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        let init = Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap();
        assert!(init.status.success());

        let runtime = runtime(&temp.path().join("sender-data"));
        fs::create_dir_all(&runtime.root).unwrap();
        let config = AppConfig {
            repository_url: remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            role: DeviceRole::Sender,
            destination: None,
        };
        let repository = GitRepository::new(runtime.repository.clone(), &config).unwrap();
        repository.ensure().unwrap();

        let source = temp.path().join("document.docx");
        let original = b"PK\x03\x04original document bytes";
        fs::write(&source, original).unwrap();
        let desired = temp.path().join("desired");
        fs::create_dir_all(&desired).unwrap();
        fs::write(desired.join("document.docx"), original).unwrap();
        let manifest = build_manifest(&desired, None).unwrap();

        let files_root = runtime.repository.join("files");
        fs::create_dir_all(&files_root).unwrap();
        fs::write(
            files_root.join("document.docx"),
            b"PK\x03\x04filtered bytes",
        )
        .unwrap();
        let metadata_root = runtime.repository.join(".filesync");
        fs::create_dir_all(&metadata_root).unwrap();
        write_json_atomic(&metadata_root.join("manifest.json"), &manifest).unwrap();
        fs::write(
            runtime.repository.join(".gitattributes"),
            "/files/** -text\n",
        )
        .unwrap();
        repository.stage_all().unwrap();
        repository.commit("Legacy corrupt commit").unwrap();
        repository.push_head().unwrap();

        let result =
            publish_files(&runtime, &config, &mut LocalState::default(), vec![source]).unwrap();
        assert!(!result.pending_push);
        assert_eq!(
            repository
                .read_blob(result.commit.as_deref().unwrap(), "files/document.docx")
                .unwrap(),
            original
        );
    }

    #[test]
    fn publishes_pulls_resolves_conflicts_and_applies_deletions() {
        let temp = tempdir().unwrap();
        let remote = temp.path().join("remote.git");
        let init = Command::new("git")
            .args(["init", "--bare", "--initial-branch=main"])
            .arg(&remote)
            .output()
            .unwrap();
        assert!(
            init.status.success(),
            "{}",
            String::from_utf8_lossy(&init.stderr)
        );

        let legacy_contents = b"existing repository file";
        let legacy = temp.path().join("legacy");
        fs::create_dir_all(&legacy).unwrap();
        let legacy_init = Command::new("git")
            .args(["init", "--initial-branch=main"])
            .arg(&legacy)
            .output()
            .unwrap();
        assert!(legacy_init.status.success());
        fs::write(legacy.join("legacy.txt"), legacy_contents).unwrap();
        for args in [
            vec!["config", "user.name", "katcoo"],
            vec!["config", "user.email", "katcoo@localhost"],
            vec!["add", "legacy.txt"],
            vec!["commit", "-m", "Existing repository"],
        ] {
            let output = Command::new("git")
                .args(args)
                .current_dir(&legacy)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let add_remote = Command::new("git")
            .args(["remote", "add", "origin"])
            .arg(&remote)
            .current_dir(&legacy)
            .output()
            .unwrap();
        assert!(add_remote.status.success());
        let seed_push = Command::new("git")
            .args(["push", "origin", "main"])
            .current_dir(&legacy)
            .output()
            .unwrap();
        assert!(
            seed_push.status.success(),
            "{}",
            String::from_utf8_lossy(&seed_push.stderr)
        );

        let source_root = temp.path().join("source");
        let selected = source_root.join("Project");
        let first_contents = b"version one\r\nline two\r\n";
        fs::create_dir_all(selected.join("empty")).unwrap();
        fs::write(selected.join("note.txt"), first_contents).unwrap();

        let sender_runtime = runtime(&temp.path().join("sender-data"));
        fs::create_dir_all(&sender_runtime.root).unwrap();
        let sender_config = AppConfig {
            repository_url: remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            role: DeviceRole::Sender,
            destination: None,
        };
        let mut sender_state = LocalState::default();
        let repository =
            GitRepository::new(sender_runtime.repository.clone(), &sender_config).unwrap();
        repository.ensure().unwrap();
        let existing_commit = repository.remote_head().unwrap().unwrap();
        repository.checkout_remote(&existing_commit).unwrap();
        let before_publish = read_repository_snapshot(&sender_runtime, &sender_config).unwrap();
        assert!(before_publish.available);
        assert!(!before_publish.initialized);
        assert_eq!(before_publish.managed_file_count, 0);
        assert_eq!(before_publish.unmanaged_file_count, 1);
        assert_eq!(before_publish.files[0].path, "legacy.txt");
        assert!(!before_publish.files[0].managed);

        let first = publish_files(
            &sender_runtime,
            &sender_config,
            &mut sender_state,
            vec![selected.clone()],
        )
        .unwrap();
        assert!(!first.pending_push);
        assert!(first.message.contains("保留仓库原有文件"));

        let snapshot = read_repository_snapshot(&sender_runtime, &sender_config).unwrap();
        assert!(snapshot.available);
        assert!(snapshot.initialized);
        assert_eq!(snapshot.file_count, 2);
        assert_eq!(snapshot.folder_count, 2);
        assert_eq!(snapshot.managed_file_count, 1);
        assert_eq!(snapshot.unmanaged_file_count, 1);
        assert_eq!(
            snapshot.total_bytes,
            (first_contents.len() + legacy_contents.len()) as u64
        );
        assert_eq!(snapshot.files.len(), 2);
        assert_eq!(snapshot.files[0].path, "Project/note.txt");
        assert!(snapshot.files[0].managed);
        assert_eq!(snapshot.files[1].path, "legacy.txt");
        assert!(!snapshot.files[1].managed);
        assert_eq!(snapshot.commit.as_deref(), first.commit.as_deref());

        let destination = temp.path().join("received");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("unrelated.txt"), b"keep me").unwrap();
        let receiver_runtime = runtime(&temp.path().join("receiver-data"));
        fs::create_dir_all(&receiver_runtime.root).unwrap();
        let receiver_config = AppConfig {
            repository_url: remote.to_string_lossy().into_owned(),
            branch: "main".into(),
            role: DeviceRole::Receiver,
            destination: Some(destination.clone()),
        };
        let mut receiver_state = LocalState::default();
        let initial_plan =
            prepare_pull_plan(&receiver_runtime, &receiver_config, &mut receiver_state).unwrap();
        assert!(initial_plan.conflicts.is_empty());
        assert!(receiver_state.last_checked_at.is_some());
        assert!(receiver_state.last_sync_at.is_none());
        apply_pull_plan(
            &receiver_runtime,
            &receiver_config,
            &mut receiver_state,
            initial_plan.commit.as_deref().unwrap(),
            &[],
        )
        .unwrap();
        assert!(receiver_state.last_sync_at.is_some());
        assert_eq!(receiver_state.last_checked_at, receiver_state.last_sync_at);
        assert_eq!(
            fs::read(destination.join("Project/note.txt")).unwrap(),
            first_contents
        );
        assert!(destination.join("Project/empty").is_dir());
        assert!(!destination.join("legacy.txt").exists());

        fs::write(destination.join("Project/note.txt"), b"local edit").unwrap();
        fs::write(selected.join("note.txt"), b"version two").unwrap();
        publish_files(
            &sender_runtime,
            &sender_config,
            &mut sender_state,
            vec![selected.clone()],
        )
        .unwrap();
        let stale_delete = delete_repository_files(
            &receiver_runtime,
            &receiver_config,
            &mut receiver_state,
            vec![RepositoryDeleteTarget {
                path: "Project/note.txt".into(),
                managed: true,
            }],
            first.commit.as_deref(),
        )
        .unwrap_err();
        assert!(stale_delete.contains("已被修改"));
        let conflict_plan =
            prepare_pull_plan(&receiver_runtime, &receiver_config, &mut receiver_state).unwrap();
        assert!(conflict_plan
            .conflicts
            .iter()
            .any(|conflict| conflict.path == "Project/note.txt"));
        apply_pull_plan(
            &receiver_runtime,
            &receiver_config,
            &mut receiver_state,
            conflict_plan.commit.as_deref().unwrap(),
            &[ConflictResolution {
                path: "Project/note.txt".into(),
                action: ConflictAction::Backup,
            }],
        )
        .unwrap();
        assert_eq!(
            fs::read(destination.join("Project/note.txt")).unwrap(),
            b"version two"
        );
        let backup_found = WalkDir::new(destination.join(".gitsynctools-backups"))
            .into_iter()
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == "note.txt");
        assert!(backup_found);

        let unrelated = temp.path().join("Other.txt");
        fs::write(&unrelated, b"unrelated remote update").unwrap();
        publish_files(
            &sender_runtime,
            &sender_config,
            &mut sender_state,
            vec![unrelated],
        )
        .unwrap();

        let delete_targets = vec![
            RepositoryDeleteTarget {
                path: "Project/note.txt".into(),
                managed: true,
            },
            RepositoryDeleteTarget {
                path: "legacy.txt".into(),
                managed: false,
            },
        ];
        let deletion = delete_repository_files(
            &receiver_runtime,
            &receiver_config,
            &mut receiver_state,
            delete_targets.clone(),
            conflict_plan.commit.as_deref(),
        )
        .unwrap();
        assert_eq!(deletion.message, "已删除 2 个仓库文件");
        let repeated_deletion = delete_repository_files(
            &sender_runtime,
            &sender_config,
            &mut sender_state,
            delete_targets,
            conflict_plan.commit.as_deref(),
        )
        .unwrap();
        assert!(!repeated_deletion.changed);
        assert_eq!(repeated_deletion.message, "所选文件已在仓库中删除");
        let after_deletion = read_repository_snapshot(&receiver_runtime, &receiver_config).unwrap();
        assert_eq!(after_deletion.managed_file_count, 1);
        assert_eq!(after_deletion.unmanaged_file_count, 0);
        let delete_plan =
            prepare_pull_plan(&receiver_runtime, &receiver_config, &mut receiver_state).unwrap();
        assert!(delete_plan
            .changes
            .iter()
            .any(|change| change.path == "Project/note.txt" && change.kind == ChangeKind::Delete));
        assert!(delete_plan.conflicts.is_empty());
        apply_pull_plan(
            &receiver_runtime,
            &receiver_config,
            &mut receiver_state,
            delete_plan.commit.as_deref().unwrap(),
            &[],
        )
        .unwrap();
        assert!(!destination.join("Project/note.txt").exists());
        assert_eq!(
            fs::read(destination.join("unrelated.txt")).unwrap(),
            b"keep me"
        );
    }
}
