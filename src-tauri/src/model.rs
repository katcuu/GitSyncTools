use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;
pub const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum DeviceRole {
    Sender,
    Receiver,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub repository_url: String,
    pub branch: String,
    pub role: DeviceRole,
    pub destination: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ManifestKind {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManifestEntry {
    pub path: String,
    pub kind: ManifestKind,
    pub size: u64,
    pub sha256: Option<String>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryInfo {
    pub version: u32,
    pub repository_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppliedEntry {
    pub kind: ManifestKind,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct LocalState {
    pub last_sync_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub pending_push: bool,
    pub pending_commit: Option<String>,
    pub last_error: Option<String>,
    pub last_applied_commit: Option<String>,
    pub last_remote_commit: Option<String>,
    pub applied_entries: BTreeMap<String, AppliedEntry>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatus {
    pub configured: bool,
    pub platform: String,
    pub app_version: String,
    pub config: Option<AppConfig>,
    pub phase: String,
    pub repository_loading: bool,
    pub last_sync_at: Option<String>,
    pub last_checked_at: Option<String>,
    pub pending_push: bool,
    pub pending_commit: Option<String>,
    pub last_error: Option<String>,
    pub last_applied_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryFileInfo {
    pub path: String,
    pub size: u64,
    pub updated_at: String,
    pub managed: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySnapshot {
    pub available: bool,
    pub commit: Option<String>,
    pub file_count: usize,
    pub folder_count: usize,
    pub managed_file_count: usize,
    pub unmanaged_file_count: usize,
    pub initialized: bool,
    pub total_bytes: u64,
    pub truncated: bool,
    pub files: Vec<RepositoryFileInfo>,
    pub message: String,
}

impl RepositorySnapshot {
    pub fn empty(message: impl Into<String>) -> Self {
        Self {
            available: false,
            commit: None,
            file_count: 0,
            folder_count: 0,
            managed_file_count: 0,
            unmanaged_file_count: 0,
            initialized: false,
            total_bytes: 0,
            truncated: false,
            files: Vec::new(),
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigureInput {
    pub repository_url: String,
    pub branch: String,
    pub role: DeviceRole,
    pub destination: Option<PathBuf>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateInput {
    pub repository_url: String,
    pub branch: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryDeleteTarget {
    pub path: String,
    pub managed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteRepositoryFilesInput {
    pub entries: Vec<RepositoryDeleteTarget>,
    pub expected_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishResult {
    pub changed: bool,
    pub pending_push: bool,
    pub commit: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ChangeKind {
    Add,
    Update,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullChange {
    pub path: String,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ConflictKind {
    LocalModified,
    LocalDeleted,
    UnmanagedCollision,
    TypeChanged,
    RemoteDeletedLocalModified,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullConflict {
    pub path: String,
    pub kind: ConflictKind,
    pub remote_deleted: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullPlan {
    pub repository_empty: bool,
    pub commit: Option<String>,
    pub changes: Vec<PullChange>,
    pub conflicts: Vec<PullConflict>,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConflictAction {
    Keep,
    Backup,
    Overwrite,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictResolution {
    pub path: String,
    pub action: ConflictAction,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyPullInput {
    pub commit: String,
    pub resolutions: Vec<ConflictResolution>,
}
