export type DeviceRole = "sender" | "receiver";

export interface AppConfig {
  repositoryUrl: string;
  branch: string;
  role: DeviceRole;
  destination: string | null;
}

export interface SyncStatus {
  configured: boolean;
  platform: string;
  appVersion: string;
  config: AppConfig | null;
  phase: "idle" | "working" | "pendingPush" | "error";
  repositoryLoading: boolean;
  lastSyncAt: string | null;
  lastCheckedAt: string | null;
  pendingPush: boolean;
  pendingCommit: string | null;
  lastError: string | null;
  lastAppliedCommit: string | null;
}

export interface RepositoryFileInfo {
  path: string;
  size: number;
  updatedAt: string;
  managed: boolean;
}

export interface RepositoryDeleteTarget {
  path: string;
  managed: boolean;
}

export interface RepositorySnapshot {
  available: boolean;
  commit: string | null;
  fileCount: number;
  folderCount: number;
  managedFileCount: number;
  unmanagedFileCount: number;
  initialized: boolean;
  totalBytes: number;
  truncated: boolean;
  files: RepositoryFileInfo[];
  message: string;
}

export interface PublishResult {
  changed: boolean;
  pendingPush: boolean;
  commit: string | null;
  message: string;
}

export type ChangeKind = "add" | "update" | "delete";
export type ConflictKind =
  | "localModified"
  | "localDeleted"
  | "unmanagedCollision"
  | "typeChanged"
  | "remoteDeletedLocalModified";

export interface PullChange {
  path: string;
  kind: ChangeKind;
}

export interface PullConflict {
  path: string;
  kind: ConflictKind;
  remoteDeleted: boolean;
}

export interface PullPlan {
  repositoryEmpty: boolean;
  commit: string | null;
  changes: PullChange[];
  conflicts: PullConflict[];
  message: string;
}

export type ConflictAction = "keep" | "backup" | "overwrite";

export interface ConflictResolution {
  path: string;
  action: ConflictAction;
}
