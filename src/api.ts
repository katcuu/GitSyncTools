import { invoke } from "@tauri-apps/api/core";
import type { AppConfig, ConflictResolution, DeviceRole, PublishResult, PullPlan, RepositoryDeleteTarget, RepositorySnapshot, SyncStatus } from "./types";

export const previewMode = import.meta.env.DEV && typeof window !== "undefined" && !("__TAURI_INTERNALS__" in window);
let previewConfig: AppConfig | null = null;
const previewDeletedFiles = new Set<string>();

function previewStatus(): SyncStatus {
  return {
    configured: previewConfig !== null,
    platform: "windows",
    appVersion: "0.3.11",
    config: previewConfig,
    phase: "idle",
    repositoryLoading: false,
    lastSyncAt: null,
    lastCheckedAt: null,
    pendingPush: false,
    pendingCommit: null,
    lastError: null,
    lastAppliedCommit: null,
  };
}

function previewRepository(): RepositorySnapshot {
  const files = [
    { path: "Documents/项目说明.docx", size: 148224, updatedAt: "2026-07-13T10:30:00Z", managed: true },
    { path: "Documents/配置.json", size: 1812, updatedAt: "2026-07-13T10:28:00Z", managed: true },
    { path: "README.md", size: 18412, updatedAt: "2026-07-13T10:25:00Z", managed: false },
  ].filter((file) => !previewDeletedFiles.has(`${file.managed}:${file.path}`));
  const managedFileCount = files.filter((file) => file.managed).length;
  const unmanagedFileCount = files.length - managedFileCount;
  return {
    available: true,
    commit: "preview",
    fileCount: files.length,
    folderCount: 2,
    managedFileCount,
    unmanagedFileCount,
    initialized: true,
    totalBytes: files.reduce((total, file) => total + file.size, 0),
    truncated: false,
    files,
    message: unmanagedFileCount > 0
      ? `仓库中有 ${managedFileCount} 个同步文件，另有 ${unmanagedFileCount} 个未纳入同步的仓库文件`
      : `仓库中共有 ${managedFileCount} 个同步文件`,
  };
}

export const api = {
  status: () => previewMode ? Promise.resolve(previewStatus()) : invoke<SyncStatus>("get_sync_status"),
  clearLastError: () => previewMode ? Promise.resolve() : invoke<void>("clear_last_error"),
  updateProxy: () => previewMode ? Promise.resolve(null) : invoke<string | null>("get_update_proxy"),
  recordUpdateEvent: (stage: string, detail: string | null, durationMs: number) => previewMode
    ? Promise.resolve()
    : invoke<void>("record_update_event", { stage, detail, durationMs: Math.max(0, Math.round(durationMs)) }),
  openLogDirectory: () => previewMode ? Promise.resolve() : invoke<void>("open_log_directory"),
  repositorySnapshot: () => previewMode
    ? Promise.resolve(previewRepository())
    : invoke<RepositorySnapshot>("get_repository_snapshot"),
  refreshRepository: () => previewMode
    ? Promise.resolve(previewRepository())
    : invoke<RepositorySnapshot>("refresh_repository"),
  configure: (input: { repositoryUrl: string; branch: string; role: DeviceRole; destination: string | null }) => {
    if (previewMode) {
      previewConfig = input;
      return Promise.resolve(input);
    }
    return invoke<AppConfig>("configure_repository", { input });
  },
  validate: (repositoryUrl: string, branch: string) => previewMode
    ? Promise.resolve("连接成功")
    : invoke<string>("validate_connection", { input: { repositoryUrl, branch } }),
  publish: (paths: string[]) => previewMode
    ? Promise.resolve({ changed: paths.length > 0, pendingPush: false, commit: "preview", message: "同步完成" })
    : invoke<PublishResult>("publish", { paths }),
  deleteRepositoryFiles: (entries: RepositoryDeleteTarget[], expectedCommit: string | null) => {
    if (previewMode) {
      entries.forEach((entry) => previewDeletedFiles.add(`${entry.managed}:${entry.path}`));
      return Promise.resolve({ changed: entries.length > 0, pendingPush: false, commit: "preview", message: `已删除 ${entries.length} 个仓库文件` });
    }
    return invoke<PublishResult>("delete_repository_files", { input: { entries, expectedCommit } });
  },
  retryPush: () => previewMode
    ? Promise.resolve({ changed: true, pendingPush: false, commit: "preview", message: "重新上传成功" })
    : invoke<PublishResult>("retry_pending_push"),
  preparePull: () => previewMode
    ? Promise.resolve({ repositoryEmpty: false, commit: "preview", changes: [], conflicts: [], message: "已经是最新版本" })
    : invoke<PullPlan>("prepare_pull"),
  applyPull: (commit: string, resolutions: ConflictResolution[]) => previewMode
    ? Promise.resolve(previewStatus())
    : invoke<SyncStatus>("apply_pull", { input: { commit, resolutions } }),
};
