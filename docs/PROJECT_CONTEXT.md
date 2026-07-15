# GitSyncTools 项目上下文与 AI 交接手册

本文是项目的“可移植记忆”。当 ChatGPT/Codex 任务记录无法同步到另一台电脑时，新任务应先阅读本文和根目录 `AGENTS.md`，再查看代码与最近提交，不需要依赖旧聊天记录。

## 1. 产品目标与边界

GitSyncTools 用一个专用私有 Git 仓库同步普通小文件，同时隐藏 commit、push、pull 等概念：

- Windows 10/11 x64 通常作为发送端：选择文件/文件夹后发布。
- Apple Silicon macOS 通常作为接收端：把仓库内容应用到固定目录。
- 两端均可查看和批量删除仓库文件，也可从托盘执行同步。
- 首要目标是操作简单；首版边界仍是不做后台监听、通用双向合并、Git LFS 和端到端加密。
- 单文件上限为 50 MB。

源码仓库为 `https://github.com/katcuu/GitSyncTools`，默认分支为 `main`。

## 2. 技术栈和代码地图

- Tauri 2：桌面窗口、托盘、命令调用、安装包和更新器。
- Rust：Git 调用、文件复制、清单、冲突、安全校验、日志和系统集成。
- React 19 + TypeScript + Vite：设置页、发送端、接收端、仓库列表和更新界面。

主要文件：

| 路径 | 职责 |
| --- | --- |
| `src/App.tsx` | 主界面和发送/接收端交互 |
| `src/api.ts` | 前端到 Tauri command 的集中接口 |
| `src/updater.ts` | GitHub Release 更新检测、代理、下载和安装 |
| `src-tauri/src/commands.rs` | Tauri command、状态协调和后台加载 |
| `src-tauri/src/sync.rs` | 发布、拉取、清单、冲突、删除和文件安全核心 |
| `src-tauri/src/git.rs` | 隐藏 Git 工作区和无窗口 Git 子进程封装 |
| `src-tauri/src/diagnostics.rs` | 脱敏、耗时和循环日志 |
| `src-tauri/src/proxy.rs` | 环境变量及 macOS 系统代理读取 |
| `src-tauri/src/storage.rs` | 配置、状态和应用数据路径 |
| `src-tauri/src/lib.rs` | Tauri 初始化、托盘菜单、关闭到后台 |
| `.github/workflows/ci.yml` | main/PR 自动测试 |
| `.github/workflows/release.yml` | 标签触发 Windows/macOS 签名发布 |
| `scripts/build-windows.ps1` | Windows 指定标签的一键本地构建 |
| `scripts/build-macos.sh` | macOS 指定标签的一键本地构建 |

## 3. 数据和仓库模型

用户配置的同步仓库格式：

```text
files/
.filesync/manifest.json
.filesync/repository.json
.gitattributes
```

`manifest.json` 是接收端应用文件的依据，包含路径、文件/目录类型、大小、SHA-256、更新时间，并显式记录空目录。

应用内部维护隐藏 Git 工作区：

- Tauri identifier 为 `com.katcc.lightsync`，这是历史兼容项。
- Windows 常见应用数据位于 `%APPDATA%/com.katcc.lightsync/`，日志位于 `%LOCALAPPDATA%/com.katcc.lightsync/logs/`。
- macOS 数据和日志由 Tauri 对应的 app data/app log 目录提供。
- 发送端的可打开同步目录是内部仓库的 `files/`；接收端是用户配置的 destination。
- 用户选择的落盘目录中不生成 `.git`。

本地状态会记录最后检查时间、最后应用时间、最后提交、文件哈希和待推送提交。网络中断产生待推送提交后，在处理完成前不接受下一批发布。

## 4. 必须保持的同步不变量

### 发送端

每次发布前先获取远端状态。远端变化必须先纳入本地操作；不能 force push 或静默覆盖未知提交。同名删除已经由另一端完成时应视为幂等成功。

选择文件后复制到内部 `files/`，然后必须：

1. 使用标准 `git add` 暂存。
2. 从 Git 索引读取最终 blob。
3. 用最终 blob 的大小和 SHA-256 修正 manifest。
4. 单独重新暂存 manifest 并再次验证。
5. commit 和 push。

这里不能使用 `git hash-object` + `git update-index --cacheinfo` 绕过 `git add`。企业 Windows 环境中的透明加密软件会按读取进程识别 `git.exe`，标准 Git 暂存能得到解密内容，而应用直接读取磁盘可能得到密文。该规则适用于 docx、xlsx、pdf、图片、二进制文件等全部类型。

日志出现 `operation=staged_content_transformed ... canonical=git_index` 表示文件被系统或 Git 处理过，索引对象已成为清单真值。这通常是预期行为，不应再次“修复”为工作区字节。

### 接收端

- 内部克隆可以以远端为准，但不能直接 reset 用户的 destination。
- 文件写入使用临时文件加替换，避免半写入。
- 只删除曾由清单管理、且本地未修改的路径；无关文件必须保留。
- 本地改动冲突保留四种选择：跳过、备份后应用、覆盖、取消整次更新。
- 应用成功后才更新最后提交和已应用哈希。

### 文件安全

- 所有清单路径都要拒绝绝对路径和 `..` 穿越。
- 拒绝符号链接和 Windows junction，不跟随选择目录之外的内容。
- 校验 Git blob 与 manifest；损坏清单不得继续落盘。
- 凭据、令牌、真实本地路径不得进入日志或同步仓库元数据。

## 5. 界面和系统集成约定

- 关闭窗口时隐藏到系统托盘；托盘菜单提供打开应用、同步、打开同步文件夹和退出。
- 发送端无待选文件时，“同步”表示从远端刷新；有待选文件时表示上传。
- 接收端“立即更新”要即时呈现忙碌状态，并区分“上次检查”和“上次应用更新”。
- Windows Git 子进程使用无窗口标志，不能反复弹出 Terminal。
- 主界面和连接设置都应保持非专业用户可理解，不暴露 Git 术语作为主要操作。

## 6. 更新、代理和日志

自动更新固定读取：

```text
https://github.com/katcuu/GitSyncTools/releases/latest/download/latest.json
```

更新包必须通过 Tauri 公钥验签。私钥只允许放在 GitHub Actions Secret `TAURI_SIGNING_PRIVATE_KEY`，不要写入源码、文档、日志或聊天内容。可选密码使用 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。

更新器读取 `HTTP_PROXY`、`HTTPS_PROXY`、`NO_PROXY` 和 macOS 系统 HTTP/HTTPS/SOCKS 代理，并使用系统证书信任。Git 文件同步使用系统 Git 自身的 SSH、凭据和代理配置。

诊断日志记录应用操作、Git 子命令、结果和耗时，敏感内容必须脱敏。日志采用两个约 10 MB 文件轮转，总量约 20 MB。

## 7. 测试与发布

常规验证命令见根目录 `AGENTS.md`。同步核心变更必须增加 Rust 回归测试；前端状态和路径逻辑应增加 Vitest 测试。

发布一个版本时：

1. 同步修改 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`、`src-tauri/tauri.conf.json`、`src/api.ts`、README 徽章和 CHANGELOG。
2. 运行全部测试、构建、fmt 和 clippy。
3. 提交到 `main`，创建带注释标签 `vX.Y.Z`，推送 main 和标签。
4. 确认 GitHub Actions 的 Windows、macOS、publish 三个任务成功。
5. 检查 Release 同时包含 Windows exe、签名、macOS app.tar.gz、签名、dmg 和 `latest.json`。

完整细节见 `docs/RELEASE.md`。不要删除或重写已经发布的标签，也不要上传旧仓库的私密提交历史。

## 8. 已知历史坑点

- v0.3.11 曾直接暂存磁盘原始字节，绕过企业透明解密，导致 macOS 收到密文；v0.3.12 已恢复标准 Git 流程。不要重新引入这种实现。
- Git 对象和清单不一致时，先分别记录 Git blob 与清单的大小/SHA-256，再判断是旧版坏提交、LFS 指针还是透明处理，不能只看工作区文件。
- 配置保存曾因同步 clone/fetch 阻塞界面；现有后台仓库加载和连接检测缓存不要轻易改回同步等待。
- 软件更新与文件同步是两条独立网络路径：前者是 Tauri HTTP，后者是系统 Git，排查代理时必须分别记录。
- `com.katcc.lightsync` 与当前产品名不一致是为了沿用配置、日志和升级路径，不是待手工改名的小问题。

## 9. 换电脑继续开发

在新电脑上只需要克隆 GitHub 仓库；聊天记录无需迁移：

```bash
git clone git@github.com:katcuu/GitSyncTools.git
cd GitSyncTools
git checkout main
git pull --ff-only
```

创建新的 ChatGPT/Codex 任务后，可直接发送：

> 请继续开发这个 GitSyncTools 项目。先完整阅读根目录 AGENTS.md、docs/PROJECT_CONTEXT.md、CONTRIBUTING.md，并查看 git status、最近 10 条提交和相关源码。遵守文档中的同步、安全、企业加密兼容、测试和发布约束。先复述你理解的当前架构及本次修改可能影响的不变量，然后实施以下需求：……

为提高交接质量，每次结束开发都应：

- 把代码、测试和必要文档提交并推送到 GitHub。
- 在 CHANGELOG 或本文“已知历史坑点”补充会影响后续设计的重要结论。
- 不把临时调试数据当作项目记忆；有价值的结论写进文档或测试。
- 新任务开始时先执行 `git status` 和 `git log`，以仓库状态为准，不盲信聊天摘要。
