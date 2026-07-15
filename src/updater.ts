import { confirm } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type DownloadEvent } from "@tauri-apps/plugin-updater";

export type UpdatePhase =
  | "idle"
  | "checking"
  | "latest"
  | "available"
  | "downloading"
  | "installing"
  | "error";

export interface UpdateUiState {
  phase: UpdatePhase;
  label: string;
  detail?: string;
  progress?: number;
}

export const idleUpdateState: UpdateUiState = {
  phase: "idle",
  label: "检测更新",
};

export function updateProgress(downloaded: number, total?: number): number | undefined {
  if (!total || total <= 0) return undefined;
  return Math.min(100, Math.max(0, Math.round((downloaded / total) * 100)));
}

export function friendlyUpdateError(reason: unknown): string {
  const detail = reason instanceof Error ? reason.message : String(reason);
  const normalized = detail.toLowerCase();
  if (
    normalized.includes("error sending request")
    || normalized.includes("failed to connect")
    || normalized.includes("network")
  ) {
    return "无法连接 GitHub 更新服务器；这不影响文件同步。请检查网络或系统代理设置后重试。";
  }
  if (normalized.includes("401") || normalized.includes("403")) {
    return "GitHub Release 不允许匿名下载。请确认仓库为公开仓库且 Release 资产可以直接访问。";
  }
  if (normalized.includes("404") || normalized.includes("not found")) {
    return "GitHub 中尚未发布可用的 Release 或 latest.json。当前版本仍可正常使用。";
  }
  return `检测更新失败，但不影响文件同步：${detail}`;
}

export async function checkAndInstallUpdate(
  preview: boolean,
  setState: (state: UpdateUiState) => void,
): Promise<void> {
  setState({ phase: "checking", label: "正在检测" });
  if (preview) {
    setState({ phase: "latest", label: "已是最新版", detail: "当前版本已经是最新版本" });
    return;
  }

  try {
    const update = await check({ timeout: 20_000 });
    if (!update) {
      setState({ phase: "latest", label: "已是最新版", detail: "当前版本已经是最新版本" });
      return;
    }

    setState({
      phase: "available",
      label: `发现 v${update.version}`,
      detail: update.body || `检测到 GitSyncTools ${update.version}`,
    });
    const notes = update.body?.trim().slice(0, 800);
    const accepted = await confirm(
      [`发现新版本 ${update.version}。`, notes, "是否立即下载并安装？"].filter(Boolean).join("\n\n"),
      {
        title: "GitSyncTools 软件更新",
        kind: "info",
        okLabel: "下载并安装",
        cancelLabel: "稍后",
      },
    );
    if (!accepted) {
      await update.close();
      setState({ phase: "available", label: `可更新 v${update.version}`, detail: "已暂缓本次更新" });
      return;
    }

    let downloaded = 0;
    let total: number | undefined;
    const onDownload = (event: DownloadEvent) => {
      if (event.event === "Started") {
        total = event.data.contentLength;
      } else if (event.event === "Progress") {
        downloaded += event.data.chunkLength;
      }
      const progress = updateProgress(downloaded, total);
      setState({
        phase: "downloading",
        label: progress === undefined ? "正在下载" : `下载 ${progress}%`,
        detail: "更新包会先完成数字签名校验，再开始安装",
        progress,
      });
    };

    setState({ phase: "downloading", label: "正在下载", detail: "正在获取更新包" });
    await update.downloadAndInstall(onDownload, { timeout: 10 * 60_000 });
    setState({ phase: "installing", label: "正在安装", detail: "安装完成后应用将重新启动" });
    await relaunch();
  } catch (reason) {
    setState({ phase: "error", label: "更新失败", detail: friendlyUpdateError(reason) });
  }
}
