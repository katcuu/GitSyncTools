import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Clock3,
  CloudDownload,
  Database,
  File,
  Folder,
  FolderOpen,
  HardDrive,
  LoaderCircle,
  Plus,
  RefreshCw,
  RotateCw,
  Send,
  Settings,
  ShieldAlert,
  Sparkles,
  Trash2,
  UploadCloud,
  X,
} from "lucide-react";
import { api, previewMode } from "./api";
import { duplicateTopNames, topName } from "./pathUtils";
import { checkAndInstallUpdate, idleUpdateState, type UpdateUiState } from "./updater";
import type {
  ConflictAction,
  ConflictResolution,
  DeviceRole,
  PullConflict,
  PullPlan,
  RepositoryDeleteTarget,
  RepositorySnapshot,
  SyncStatus,
} from "./types";

const emptyStatus: SyncStatus = {
  configured: false,
  platform: "unknown",
  appVersion: "",
  config: null,
  phase: "idle",
  repositoryLoading: false,
  lastSyncAt: null,
  lastCheckedAt: null,
  pendingPush: false,
  pendingCommit: null,
  lastError: null,
  lastAppliedCommit: null,
};

const emptyRepository: RepositorySnapshot = {
  available: false,
  commit: null,
  fileCount: 0,
  folderCount: 0,
  managedFileCount: 0,
  unmanagedFileCount: 0,
  initialized: false,
  totalBytes: 0,
  truncated: false,
  files: [],
  message: "仓库中暂无同步文件",
};

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

function formatTime(value: string | null, emptyLabel = "尚未同步") {
  if (!value) return emptyLabel;
  const date = new Date(value);
  return Number.isNaN(date.getTime()) ? value : date.toLocaleString("zh-CN", { hour12: false });
}

function yieldForPaint() {
  return new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
}

function OperationMessage({ error, message, onDismissError }: {
  error: string | null;
  message: string | null;
  onDismissError: () => void;
}) {
  if (!error && !message) return null;
  return (
    <div className={error ? "inline-error" : "inline-success"}>
      {error ? <AlertCircle size={17} /> : <Check size={17} />}
      <span className="inline-message-text">{error || message}</span>
      {error && (
        <button className="inline-message-close" title="关闭" aria-label="关闭错误提示" onClick={onDismissError}>
          <X size={15} />
        </button>
      )}
    </div>
  );
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function StatusPill({ status }: { status: SyncStatus }) {
  if (status.repositoryLoading) {
    return <span className="status-pill warning"><LoaderCircle className="spin" size={14} />正在读取仓库</span>;
  }
  if (status.pendingPush) {
    return <span className="status-pill warning"><Clock3 size={14} />等待上传</span>;
  }
  if (status.phase === "error") {
    return <span className="status-pill danger"><AlertCircle size={14} />需要处理</span>;
  }
  return <span className="status-pill success"><Check size={14} />运行正常</span>;
}

interface RepositoryContentsProps {
  snapshot: RepositorySnapshot;
  allowDelete?: boolean;
  disabled?: boolean;
  onDelete?: (entries: RepositoryDeleteTarget[]) => Promise<boolean>;
}

function repositoryFileKey(file: RepositoryDeleteTarget) {
  return `${file.managed ? "managed" : "repository"}:${file.path}`;
}

function RepositoryContents({ snapshot, allowDelete = false, disabled = false, onDelete }: RepositoryContentsProps) {
  const [selectionMode, setSelectionMode] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [confirmingDelete, setConfirmingDelete] = useState(false);
  const selectedEntries = snapshot.files.filter((file) => selected.has(repositoryFileKey(file)));
  const allSelected = snapshot.files.length > 0 && selectedEntries.length === snapshot.files.length;

  useEffect(() => {
    const available = new Set(snapshot.files.map(repositoryFileKey));
    setSelected((current) => new Set([...current].filter((key) => available.has(key))));
    if (snapshot.files.length === 0) {
      setSelectionMode(false);
      setConfirmingDelete(false);
    }
  }, [snapshot.commit, snapshot.files.length]);

  function toggleSelection(file: RepositoryDeleteTarget) {
    const key = repositoryFileKey(file);
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });
    setConfirmingDelete(false);
  }

  function toggleSelectionMode() {
    if (selectionMode) setSelected(new Set());
    setConfirmingDelete(false);
    setSelectionMode(!selectionMode);
  }

  function toggleSelectAll() {
    setSelected(allSelected ? new Set() : new Set(snapshot.files.map(repositoryFileKey)));
    setConfirmingDelete(false);
  }

  async function deleteSelected() {
    if (!onDelete || selectedEntries.length === 0) return;
    const completed = await onDelete(selectedEntries.map(({ path, managed }) => ({ path, managed })));
    if (completed) {
      setSelected(new Set());
      setSelectionMode(false);
      setConfirmingDelete(false);
    }
  }

  return (
    <section className="repository-section" aria-label="仓库内容">
      <div className="repository-heading">
        <div className="repository-title"><Database size={19} /><div><h3>仓库内容</h3><p>{snapshot.message}</p></div></div>
        {snapshot.available && (
          <div className="repository-summary">
            <span>{snapshot.fileCount} 个文件</span>
            <span>{snapshot.folderCount} 个文件夹</span>
            {snapshot.unmanagedFileCount > 0 && <span className="unmanaged-summary">{snapshot.unmanagedFileCount} 个未纳入同步</span>}
            <span>{formatBytes(snapshot.totalBytes)}</span>
          </div>
        )}
      </div>
      {allowDelete && snapshot.files.length > 0 && (
        <div className="repository-toolbar">
          <button className="button secondary small" disabled={disabled} onClick={toggleSelectionMode}>
            {selectionMode ? "取消多选" : "多选"}
          </button>
          {selectionMode && (
            <>
              <button
                className="button secondary small"
                disabled={disabled}
                onClick={toggleSelectAll}
              >{allSelected ? "取消全选" : "全选"}</button>
              <span className="repository-selection-count">已选择 {selectedEntries.length} 项</span>
              <button className="button danger small" disabled={disabled || selectedEntries.length === 0} onClick={() => setConfirmingDelete(true)}>
                <Trash2 size={15} />删除{selectedEntries.length > 0 ? ` (${selectedEntries.length})` : ""}
              </button>
            </>
          )}
        </div>
      )}
      {confirmingDelete && (
        <div className="repository-delete-confirm" role="alert">
          <span>确定删除选中的 {selectedEntries.length} 个文件吗？同步目录会在下次立即更新时应用这些删除。</span>
          <button className="button secondary small" disabled={disabled} onClick={() => setConfirmingDelete(false)}>取消</button>
          <button className="button danger small" disabled={disabled} onClick={deleteSelected}><Trash2 size={15} />确认删除</button>
        </div>
      )}
      {snapshot.files.length > 0 ? (
        <div className="repository-files">
          {snapshot.files.map((file) => (
            <div className={`repository-file ${selectionMode ? "selection-mode" : ""} ${selected.has(repositoryFileKey(file)) ? "selected" : ""}`} key={repositoryFileKey(file)}>
              {selectionMode && (
                <input
                  className="repository-file-checkbox"
                  type="checkbox"
                  checked={selected.has(repositoryFileKey(file))}
                  disabled={disabled}
                  aria-label={`选择 ${file.path}`}
                  onChange={() => toggleSelection(file)}
                />
              )}
              <File size={16} />
              <strong>{file.path}</strong>
              <span className={`repository-file-kind ${file.managed ? "managed" : "unmanaged"}`}>{file.managed ? "已同步" : "仓库文件"}</span>
              <span>{formatBytes(file.size)}</span>
              <time>{formatTime(file.updatedAt)}</time>
            </div>
          ))}
        </div>
      ) : (
        <div className="repository-empty">暂无可显示的文件</div>
      )}
      {snapshot.truncated && <p className="repository-truncated">文件较多，仅显示前 500 项</p>}
    </section>
  );
}

interface SetupProps {
  status: SyncStatus;
  onSaved: () => Promise<void>;
  onCancel?: () => void;
}

function Setup({ status, onSaved, onCancel }: SetupProps) {
  const defaultRole: DeviceRole = status.config?.role ?? (status.platform === "macos" ? "receiver" : "sender");
  const [repositoryUrl, setRepositoryUrl] = useState(status.config?.repositoryUrl ?? "");
  const [branch, setBranch] = useState(status.config?.branch ?? "main");
  const [role, setRole] = useState<DeviceRole>(defaultRole);
  const [destination, setDestination] = useState(status.config?.destination ?? "");
  const [busyAction, setBusyAction] = useState<"validate" | "save" | null>(null);
  const [connectionMessage, setConnectionMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const busy = busyAction !== null;

  async function chooseDestination() {
    if (previewMode) {
      setDestination("C:\\Users\\me\\GitSyncTools");
      return;
    }
    const selected = await open({ directory: true, multiple: false, title: "选择同步目录" });
    if (typeof selected === "string") setDestination(selected);
  }

  async function validate() {
    setBusyAction("validate");
    setError(null);
    try {
      const message = await api.validate(repositoryUrl.trim(), branch.trim() || "main");
      setConnectionMessage(message);
    } catch (reason) {
      setConnectionMessage(null);
      setError(errorText(reason));
    } finally {
      setBusyAction(null);
    }
  }

  async function save() {
    if (!repositoryUrl.trim()) return setError("请输入仓库地址");
    if (role === "receiver" && !destination) return setError("请选择同步目录");
    setBusyAction("save");
    setError(null);
    try {
      await api.configure({
        repositoryUrl: repositoryUrl.trim(),
        branch: branch.trim() || "main",
        role,
        destination: role === "receiver" ? destination : null,
      });
      await onSaved();
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusyAction(null);
    }
  }

  return (
    <main className="setup-shell">
      <section className="setup-panel" aria-label="连接设置">
        <div className="brand-lockup">
          <div className="brand-mark"><RefreshCw size={22} /></div>
          <div><h1>GitSyncTools</h1><p>连接私有 Git 仓库</p></div>
          <span className="setup-version">v{status.appVersion || "-"}</span>
        </div>

        <div className="form-grid">
          <label className="field field-wide">
            <span>仓库地址</span>
            <input value={repositoryUrl} onChange={(event) => { setRepositoryUrl(event.target.value); setConnectionMessage(null); }} placeholder="git@host:owner/repository.git" />
          </label>
          <label className="field">
            <span>分支</span>
            <input value={branch} onChange={(event) => { setBranch(event.target.value); setConnectionMessage(null); }} placeholder="main" />
          </label>
          <div className="field">
            <span>本机角色</span>
            <div className="segmented" role="group" aria-label="本机角色">
              <button className={role === "sender" ? "active" : ""} onClick={() => setRole("sender")}><Send size={15} />发送端</button>
              <button className={role === "receiver" ? "active" : ""} onClick={() => setRole("receiver")}><HardDrive size={15} />接收端</button>
            </div>
          </div>
          {role === "receiver" && (
            <div className="field field-wide">
              <span>同步目录</span>
              <button className="path-picker" onClick={chooseDestination}>
                <FolderOpen size={17} /><span>{destination || "选择目录"}</span><ChevronRight size={16} />
              </button>
            </div>
          )}
        </div>

        {error && <div className="inline-error"><AlertCircle size={17} /><span>{error}</span></div>}
        {connectionMessage && <div className="inline-success"><Check size={17} /><span>{connectionMessage}</span></div>}

        <div className="form-actions">
          <button className="button secondary" disabled={busy} onClick={() => api.openLogDirectory().catch((reason) => setError(errorText(reason)))}>
            <FolderOpen size={17} />打开日志目录
          </button>
          {onCancel && <button className="button secondary" onClick={onCancel}>取消</button>}
          <button className="button secondary" disabled={busy || !repositoryUrl.trim()} onClick={validate}>
            {busyAction === "validate" ? <LoaderCircle className="spin" size={17} /> : <ShieldAlert size={17} />}{busyAction === "validate" ? "检测中" : "检测连接"}
          </button>
          <button className="button primary" disabled={busy} onClick={save}>
            {busyAction === "save" ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />}{busyAction === "save" ? "保存中" : "保存设置"}
          </button>
        </div>
      </section>
    </main>
  );
}

interface SenderProps {
  status: SyncStatus;
  repository: RepositorySnapshot;
  refresh: () => Promise<void>;
  openSettings: () => void;
}

function Sender({ status, repository, refresh, openSettings }: SenderProps) {
  const [paths, setPaths] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(status.lastError);

  const duplicates = useMemo(() => duplicateTopNames(paths), [paths]);

  useEffect(() => setError(status.lastError), [status.lastError]);

  async function dismissError() {
    setError(null);
    try {
      await api.clearLastError();
      await refresh();
    } catch {
      // The local notice is already dismissed; a later refresh can restore a persistent error.
    }
  }

  const addPaths = useCallback((incoming: string[]) => {
    setMessage(null);
    setError(null);
    setPaths((current) => [...new Set([...current, ...incoming])]);
  }, []);

  useEffect(() => {
    if (previewMode) return;
    let unlisten: (() => void) | undefined;
    getCurrentWebviewWindow().onDragDropEvent((event) => {
      if (event.payload.type === "drop") addPaths(event.payload.paths);
    }).then((dispose) => { unlisten = dispose; });
    return () => unlisten?.();
  }, [addPaths]);

  async function chooseFiles() {
    const selected = await open({ multiple: true, directory: false, title: "选择文件" });
    if (typeof selected === "string") addPaths([selected]);
    else if (selected) addPaths(selected);
  }

  async function chooseFolder() {
    const selected = await open({ multiple: false, directory: true, title: "选择文件夹" });
    if (typeof selected === "string") addPaths([selected]);
  }

  async function publish() {
    if (!paths.length || duplicates.size) return;
    setBusy(true);
    setError(null);
    try {
      const result = await api.publish(paths);
      setMessage(result.message);
      if (!result.pendingPush) setPaths([]);
      await refresh();
    } catch (reason) {
      setError(errorText(reason));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function refreshRepository() {
    setBusy(true);
    setMessage(null);
    setError(null);
    await yieldForPaint();
    try {
      await api.refreshRepository();
      setMessage("仓库信息已更新");
      await refresh();
    } catch (reason) {
      setError(errorText(reason));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function retry() {
    setBusy(true);
    setError(null);
    try {
      const result = await api.retryPush();
      setMessage(result.message);
      if (!result.pendingPush) setPaths([]);
      await refresh();
    } catch (reason) {
      setError(errorText(reason));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function openSyncDirectory() {
    setError(null);
    try {
      await api.openSyncDirectory();
    } catch (reason) {
      setError(errorText(reason));
    }
  }

  async function deleteRepositoryEntries(entries: RepositoryDeleteTarget[]): Promise<boolean> {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await api.deleteRepositoryFiles(entries, repository.commit);
      setMessage(result.message);
      await refresh();
      return true;
    } catch (reason) {
      setError(errorText(reason));
      await refresh();
      return false;
    } finally {
      setBusy(false);
    }
  }

  return (
    <AppFrame status={status} openSettings={openSettings}>
      <section className="workspace-heading">
        <div><h2>发送文件</h2><p>{status.config?.repositoryUrl}</p></div>
        <div className="workspace-heading-actions">
          <button className="button secondary small" disabled={busy} onClick={openSyncDirectory}>
            <FolderOpen size={16} />打开同步文件夹
          </button>
          <StatusPill status={status} />
        </div>
      </section>

      {status.pendingPush && (
        <div className="pending-banner">
          <div><Clock3 size={19} /><span>上次内容尚未上传</span></div>
          <button className="button warning" disabled={busy} onClick={retry}><RotateCw size={17} />重新上传</button>
        </div>
      )}

      {status.repositoryLoading && (
        <div className="pending-banner repository-loading-banner">
          <div><LoaderCircle className="spin" size={19} /><span>设置已保存，正在后台下载仓库文件；完成后列表会自动刷新</span></div>
        </div>
      )}

      <section className="drop-zone" aria-label="待同步文件">
        <UploadCloud size={30} />
        <div><h3>待同步</h3><p>{paths.length ? `已选择 ${paths.length} 项` : "暂无文件"}</p></div>
        <div className="drop-actions">
          <button className="button secondary" disabled={busy || status.pendingPush || status.repositoryLoading} onClick={chooseFiles}><Plus size={17} />选择文件</button>
          <button className="button secondary" disabled={busy || status.pendingPush || status.repositoryLoading} onClick={chooseFolder}><Folder size={17} />选择文件夹</button>
        </div>
      </section>

      {paths.length > 0 && (
        <section className="selection-list" aria-label="已选择内容">
          {paths.map((path) => {
            const duplicate = duplicates.has(topName(path).toLocaleLowerCase());
            return (
              <div className={`selection-row ${duplicate ? "invalid" : ""}`} key={path}>
                <File size={18} />
                <div><strong>{topName(path)}</strong><span>{path}</span></div>
                {duplicate && <span className="row-warning">名称重复</span>}
                <button className="icon-button" title="移除" aria-label={`移除 ${topName(path)}`} onClick={() => setPaths((current) => current.filter((item) => item !== path))}><X size={17} /></button>
              </div>
            );
          })}
        </section>
      )}

      <OperationMessage error={error} message={message} onDismissError={dismissError} />

      <footer className="action-bar">
        <div><span>上次同步</span><strong>{formatTime(status.lastSyncAt)}</strong></div>
        <button
          className="button primary large"
          disabled={busy || duplicates.size > 0 || status.pendingPush || status.repositoryLoading}
          title={paths.length ? "上传到仓库" : "从仓库更新"}
          onClick={paths.length ? publish : refreshRepository}
        >
          {busy ? <LoaderCircle className="spin" size={18} /> : paths.length ? <UploadCloud size={18} /> : <CloudDownload size={18} />}同步
        </button>
      </footer>
      <RepositoryContents
        snapshot={repository}
        allowDelete
        disabled={busy || status.pendingPush || status.repositoryLoading}
        onDelete={deleteRepositoryEntries}
      />
    </AppFrame>
  );
}

function conflictLabel(conflict: PullConflict) {
  const labels: Record<PullConflict["kind"], string> = {
    localModified: "本地文件已修改",
    localDeleted: "本地文件已删除",
    unmanagedCollision: "本地已有同名内容",
    typeChanged: "文件类型发生变化",
    remoteDeletedLocalModified: "远端已删除，本地有修改",
  };
  return labels[conflict.kind];
}

interface ConflictPanelProps {
  plan: PullPlan;
  busy: boolean;
  onCancel: () => void;
  onApply: (resolutions: ConflictResolution[]) => Promise<void>;
}

function ConflictPanel({ plan, busy, onCancel, onApply }: ConflictPanelProps) {
  const [choices, setChoices] = useState<Record<string, ConflictAction>>({});
  const complete = plan.conflicts.every((conflict) => choices[conflict.path]);

  return (
    <section className="conflict-panel">
      <div className="conflict-title"><ShieldAlert size={22} /><div><h3>需要确认本地改动</h3><p>{plan.conflicts.length} 项内容与远端不一致</p></div></div>
      <div className="conflict-list">
        {plan.conflicts.map((conflict) => (
          <div className="conflict-row" key={conflict.path}>
            <div><strong>{conflict.path}</strong><span>{conflictLabel(conflict)}</span></div>
            <div className="segmented compact" role="group" aria-label={`${conflict.path} 处理方式`}>
              <button className={choices[conflict.path] === "keep" ? "active" : ""} onClick={() => setChoices((value) => ({ ...value, [conflict.path]: "keep" }))}>保留本地</button>
              <button className={choices[conflict.path] === "backup" ? "active" : ""} onClick={() => setChoices((value) => ({ ...value, [conflict.path]: "backup" }))}>备份后更新</button>
              <button className={choices[conflict.path] === "overwrite" ? "active danger-choice" : ""} onClick={() => setChoices((value) => ({ ...value, [conflict.path]: "overwrite" }))}>{conflict.remoteDeleted ? "删除本地" : "直接覆盖"}</button>
            </div>
          </div>
        ))}
      </div>
      <div className="conflict-actions">
        <button className="button secondary" disabled={busy} onClick={onCancel}>取消更新</button>
        <button className="button primary" disabled={busy || !complete} onClick={() => onApply(plan.conflicts.map((conflict) => ({ path: conflict.path, action: choices[conflict.path] })))}>
          {busy ? <LoaderCircle className="spin" size={17} /> : <Check size={17} />}应用更新
        </button>
      </div>
    </section>
  );
}

interface ReceiverProps {
  status: SyncStatus;
  repository: RepositorySnapshot;
  refresh: () => Promise<void>;
  openSettings: () => void;
}

function Receiver({ status, repository, refresh, openSettings }: ReceiverProps) {
  const [busy, setBusy] = useState(false);
  const [busyLabel, setBusyLabel] = useState("正在检查远端");
  const [plan, setPlan] = useState<PullPlan | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(status.lastError);

  useEffect(() => setError(status.lastError), [status.lastError]);

  async function dismissError() {
    setError(null);
    try {
      await api.clearLastError();
      await refresh();
    } catch {
      // The local notice is already dismissed; a later refresh can restore a persistent error.
    }
  }

  async function apply(current: PullPlan, resolutions: ConflictResolution[]) {
    if (!current.commit) return;
    setBusy(true);
    setBusyLabel("正在应用更新");
    setError(null);
    await yieldForPaint();
    try {
      await api.applyPull(current.commit, resolutions);
      setPlan(null);
      setMessage("文件已更新");
      await refresh();
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  }

  async function update() {
    setBusy(true);
    setBusyLabel("正在检查远端");
    setMessage(null);
    setError(null);
    await yieldForPaint();
    try {
      const next = await api.preparePull();
      if (next.repositoryEmpty || !next.commit || next.changes.length === 0) {
        setMessage(next.message);
        setPlan(null);
        await refresh();
      } else if (next.conflicts.length) {
        setPlan(next);
        await refresh();
      } else {
        await apply(next, []);
      }
    } catch (reason) {
      setError(errorText(reason));
    } finally {
      setBusy(false);
    }
  }

  async function retry() {
    setBusy(true);
    setError(null);
    try {
      const result = await api.retryPush();
      setMessage(result.message);
      await refresh();
    } catch (reason) {
      setError(errorText(reason));
      await refresh();
    } finally {
      setBusy(false);
    }
  }

  async function openSyncDirectory() {
    setError(null);
    try {
      await api.openSyncDirectory();
    } catch (reason) {
      setError(errorText(reason));
    }
  }

  async function deleteRepositoryEntries(entries: RepositoryDeleteTarget[]): Promise<boolean> {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await api.deleteRepositoryFiles(entries, repository.commit);
      setMessage(result.message);
      await refresh();
      return true;
    } catch (reason) {
      setError(errorText(reason));
      await refresh();
      return false;
    } finally {
      setBusy(false);
    }
  }

  return (
    <AppFrame status={status} openSettings={openSettings}>
      <section className="workspace-heading">
        <div><h2>接收文件</h2><p>{status.config?.destination}</p></div>
        <div className="workspace-heading-actions">
          <button className="button secondary small" disabled={busy} onClick={openSyncDirectory}>
            <FolderOpen size={16} />打开同步文件夹
          </button>
          <StatusPill status={status} />
        </div>
      </section>

      {status.pendingPush && (
        <div className="pending-banner">
          <div><Clock3 size={19} /><span>文件删除尚未上传</span></div>
          <button className="button warning" disabled={busy} onClick={retry}><RotateCw size={17} />重新上传</button>
        </div>
      )}

      {plan ? (
        <ConflictPanel plan={plan} busy={busy} onCancel={() => setPlan(null)} onApply={(resolutions) => apply(plan, resolutions)} />
      ) : (
        <section className="receiver-stage">
          <div className="sync-disc"><RefreshCw className={busy ? "spin" : ""} size={44} /></div>
          <div className="receiver-status">
            <h3>{busy ? busyLabel : "准备就绪"}</h3>
            <p>上次检查：{formatTime(status.lastCheckedAt, "尚未检查")}</p>
            <p>上次应用更新：{formatTime(status.lastSyncAt, "尚未应用更新")}</p>
          </div>
          <button className="button primary large" disabled={busy || status.pendingPush || status.repositoryLoading} onClick={update}>
            {busy ? <LoaderCircle className="spin" size={18} /> : <RefreshCw size={18} />}立即更新
          </button>
        </section>
      )}

      <OperationMessage error={error} message={message} onDismissError={dismissError} />
      <RepositoryContents
        snapshot={repository}
        allowDelete
        disabled={busy || status.pendingPush || status.repositoryLoading}
        onDelete={deleteRepositoryEntries}
      />
    </AppFrame>
  );
}

function UpdateControl() {
  const [state, setState] = useState<UpdateUiState>(idleUpdateState);
  const busy = state.phase === "checking" || state.phase === "downloading" || state.phase === "installing";

  useEffect(() => {
    if (state.phase !== "latest") return;
    const timer = window.setTimeout(() => setState(idleUpdateState), 5000);
    return () => window.clearTimeout(timer);
  }, [state.phase]);

  return (
    <div className="update-control">
      <button
        className={`update-button ${state.phase}`}
        disabled={busy}
        title={state.detail || state.label}
        onClick={() => checkAndInstallUpdate(previewMode, setState)}
      >
        {busy ? <LoaderCircle className="spin" size={15} /> : <Sparkles size={15} />}
        <span>{state.label}</span>
      </button>
      {state.detail && state.phase !== "idle" && (
        <div className={`update-feedback ${state.phase}`} role="status">
          <span>{state.detail}</span>
          <button
            className="update-feedback-close"
            aria-label="关闭更新提示"
            title="关闭"
            onClick={() => setState(idleUpdateState)}
          ><X size={14} /></button>
        </div>
      )}
    </div>
  );
}

function AppFrame({ status, openSettings, children }: { status: SyncStatus; openSettings: () => void; children: React.ReactNode }) {
  return (
    <div className="app-shell">
      <header className="topbar">
        <div className="brand-small"><div className="brand-mark small"><RefreshCw size={17} /></div><strong>GitSyncTools</strong></div>
        <div className="topbar-actions">
          <UpdateControl />
          <button className="icon-button" title="连接设置" aria-label="连接设置" onClick={openSettings}><Settings size={19} /></button>
        </div>
      </header>
      <main className="workspace">{children}</main>
      <div className="repo-foot">
        <span>{status.config?.branch ?? "main"}</span>
        <span>GitSyncTools v{status.appVersion || "-"} · Git {status.platform === "macos" ? "macOS" : "Windows"}</span>
      </div>
    </div>
  );
}

export default function App() {
  const [status, setStatus] = useState<SyncStatus>(emptyStatus);
  const [repository, setRepository] = useState<RepositorySnapshot>(emptyRepository);
  const [loading, setLoading] = useState(true);
  const [editingSettings, setEditingSettings] = useState(false);
  const [fatalError, setFatalError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [statusResult, repositoryResult] = await Promise.allSettled([
        api.status(),
        api.repositorySnapshot(),
      ]);
      if (statusResult.status === "rejected") throw statusResult.reason;
      setStatus(statusResult.value);
      setRepository(repositoryResult.status === "fulfilled"
        ? repositoryResult.value
        : { ...emptyRepository, message: "暂时无法读取仓库文件信息" });
      setFatalError(null);
    } catch (reason) {
      setFatalError(errorText(reason));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    refresh();
    if (previewMode) return;
    let dispose: (() => void) | undefined;
    listen("sync-status-updated", refresh).then((unlisten) => { dispose = unlisten; });
    return () => dispose?.();
  }, [refresh]);

  if (loading) return <div className="loading-screen"><LoaderCircle className="spin" size={30} /></div>;
  if (fatalError && !status.configured) return <div className="fatal-screen"><AlertCircle size={28} /><strong>无法启动 GitSyncTools</strong><span>{fatalError}</span></div>;
  if (!status.configured || editingSettings) {
    return <Setup status={status} onSaved={async () => { setEditingSettings(false); await refresh(); }} onCancel={status.configured ? () => setEditingSettings(false) : undefined} />;
  }
  return status.config?.role === "receiver"
    ? <Receiver status={status} repository={repository} refresh={refresh} openSettings={() => setEditingSettings(true)} />
    : <Sender status={status} repository={repository} refresh={refresh} openSettings={() => setEditingSettings(true)} />;
}
