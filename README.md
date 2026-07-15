# GitSyncTools

[![CI](https://github.com/katcuu/GitSyncTools/actions/workflows/ci.yml/badge.svg)](https://github.com/katcuu/GitSyncTools/actions/workflows/ci.yml)
[![release](https://img.shields.io/badge/release-v0.3.13-176b4f)](https://github.com/katcuu/GitSyncTools/releases)
[![license](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

GitSyncTools 是一个基于 Git 仓库的轻量级单向文件同步工具。Windows 发送端负责选择并发布文件，macOS 接收端负责把远端内容更新到固定目录。应用隐藏了 `commit`、`push`、`pull` 等 Git 细节，日常操作只需要“选择文件并同步”或“立即更新”。

> 当前项目处于 `0.x` 阶段，仓库格式已版本化，但公开 API 和界面仍可能调整。

## 功能特性

- 支持拖放、多选文件、选择文件夹和空目录。
- 使用专用私有 Git 仓库保留历史版本。
- Windows 资源管理器经典右键菜单支持“同步到 GitSyncTools”。
- macOS 接收端只修改应用管理过的路径，不删除无关文件。
- 检测本地改动，并提供保留、备份、覆盖或取消更新等冲突处理方式。
- 展示仓库中的文件数量、目录数量、总大小、路径和更新时间。
- Windows 和 macOS 主界面及托盘菜单均可直接打开当前同步文件夹。
- Windows 发送端和 macOS 接收端的仓库文件列表均支持多选、全选，并可批量删除同步文件或仓库原有文件。
- 网络中断时保留待推送提交，支持后续重试。
- 关闭主窗口后驻留系统托盘；Windows 和 macOS 均可从托盘右键菜单选择“同步”。发送端会刷新仓库信息，接收端会拉取并应用最新内容，遇到冲突时自动打开主窗口。
- 支持基于 GitHub Release 的签名更新包检测、下载和安装。
- macOS 更新检测自动读取环境变量及系统 HTTPS、HTTP、SOCKS 代理，并使用系统证书信任。
- 记录更新、同步、删除及 Git 子命令耗时；日志采用两个 10 MB 文件循环存储，可从连接设置打开日志目录。
- 拒绝路径穿越、符号链接、Windows junction、损坏清单和超过 50 MB 的单文件。

## 支持平台

| 平台 | 架构 | 角色 | 状态 |
| --- | --- | --- | --- |
| Windows 10/11 | x64 | 发送端 | 已支持 |
| macOS | Apple Silicon | 接收端 | 已支持，安装包需在 macOS 构建 |

两端均需安装可用的 Git，并提前配置 SSH 密钥或 HTTPS 凭据管理器。

## 快速开始

### 1. 创建同步仓库

在 GitLab、GitHub、Gitee 或自建 Git 服务中创建一个空的私有仓库。该仓库应专供 GitSyncTools 使用，不要在网页或其他 Git 客户端中直接修改应用生成的提交。

### 2. 配置 Windows 发送端

1. 安装并启动 GitSyncTools。
2. 输入仓库 SSH 或 HTTPS 地址，默认分支为 `main`。
3. 选择“发送端”，检测连接并保存。
4. 拖入文件或文件夹，点击“同步”。

Windows 11 的资源管理器右键入口位于“显示更多选项”中。

### 3. 配置 macOS 接收端

1. 启动 GitSyncTools。
2. 输入相同仓库地址和分支。
3. 选择“接收端”和固定同步目录。
4. 点击“立即更新”。

未被 GitSyncTools 管理的本地文件不会被删除。备份文件写入同步目录下的 `.gitsynctools-backups/时间/`。

## 自动更新

主窗口右上角提供“检测更新”。发现更高版本后，应用会显示版本和发布说明，征得确认后下载更新包、验证 Tauri 签名并启动安装。

更新数据来自 GitHub 最新 Release 的固定地址：

```text
https://github.com/katcuu/GitSyncTools/releases/latest/download/latest.json
```

GitHub Actions 在推送版本标签后构建 Windows x64 和 Apple Silicon macOS 安装包，生成统一的 `latest.json` 并创建 GitHub Release。安装包仍强制校验 Tauri 数字签名，签名私钥只保存在 GitHub Actions Secrets 中。更新请求默认使用系统代理设置以及 `HTTP_PROXY`、`HTTPS_PROXY`、`NO_PROXY` 环境变量；文件同步使用的系统 Git 同样遵循 Git 自身和环境代理配置。详细发布流程见 [docs/RELEASE.md](docs/RELEASE.md)。

## 使用 AI 在另一台电脑继续开发

项目把长期上下文维护在根目录 [AGENTS.md](AGENTS.md) 和 [docs/PROJECT_CONTEXT.md](docs/PROJECT_CONTEXT.md) 中，包括架构、同步不变量、企业加密兼容、历史问题、测试发布流程和可直接使用的新任务提示词。换电脑后克隆 GitHub 仓库，让新的 ChatGPT/Codex 任务先完整阅读这两个文件，即可在没有旧聊天记录的情况下继续开发。

## 仓库格式

同步仓库结构固定为：

```text
files/
.filesync/manifest.json
.filesync/repository.json
.gitattributes
```

`manifest.json` 记录相对路径、类型、大小、SHA-256、更新时间和空目录。源设备绝对路径、用户名和凭据不会提交到同步仓库。

## 本地开发

### 环境要求

- Node.js 22+
- Rust stable 1.77.2+
- Git
- Windows C++ Build Tools，或 macOS Xcode Command Line Tools

### 安装与运行

```bash
npm ci
npm test
npm run build
npm run tauri dev
```

Rust 检查：

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

### 一键构建指定版本

项目提供 Windows 和 Apple Silicon macOS 一键构建脚本。第一个参数是要构建的 Git 标签；第二个参数控制是否生成 Tauri 自动更新产物，只接受 `true` 或 `false`，省略时默认为 `false`。

脚本会依次获取远端标签、确认标签存在、**强制删除当前仓库的所有未提交修改、未跟踪文件及被忽略文件**、切换到目标标签、执行 `npm ci` 和前端测试，然后生成安装包。不要把私钥或需要保留的文件放在项目目录内。

Windows：

```powershell
.\scripts\build-windows.ps1 v0.3.7
.\scripts\build-windows.ps1 v0.3.7 true
```

macOS：

```bash
chmod +x scripts/build-macos.sh
./scripts/build-macos.sh v0.3.7
./scripts/build-macos.sh v0.3.7 true
```

第二个参数为 `true` 时，必须提前设置 `TAURI_SIGNING_PRIVATE_KEY`；macOS 会同时构建 `app` 和 `dmg`，从而生成 `.app.tar.gz` 及其 `.sig`。脚本执行完成后仓库会停留在目标标签的 detached HEAD 状态；需要继续开发或再次使用只存在于 `main` 的脚本时，执行 `git switch main`。

### Windows 构建

```powershell
npm run tauri -- build --bundles nsis
```

Windows 安装包位于 `src-tauri/target/release/bundle/nsis/`。构建可用于自动更新的签名包前，需要把私钥内容或私钥路径写入 `TAURI_SIGNING_PRIVATE_KEY`。

### macOS Apple Silicon 打包

macOS 的 `.app`、`.dmg` 和更新包必须在 macOS 上构建，不能直接在 Windows 或 Linux 上生成。本项目首版 macOS 目标为 Apple Silicon，即 `aarch64-apple-darwin`。

Tauri 官方参考：[macOS 前置条件](https://v2.tauri.app/start/prerequisites/)、[DMG 打包](https://v2.tauri.app/distribute/dmg/)、[macOS 签名](https://v2.tauri.app/distribute/sign/macos/)、[更新包签名](https://v2.tauri.app/plugin/updater/)。

#### 1. 准备 Mac

最低要求：

- macOS Catalina 10.15 或更高版本；推荐使用仍受 Apple 支持的最新版 macOS。
- Apple Silicon Mac，终端执行 `uname -m` 应输出 `arm64`。
- 至少预留 10 GB 可用空间用于 Xcode 工具链、Rust 和构建缓存。
- 能访问项目 Git 仓库和 npm、Cargo 依赖源。

只构建桌面应用时，安装 Xcode Command Line Tools 即可：

```bash
xcode-select --install
```

安装完成后验证：

```bash
xcode-select -p
xcrun --find clang
git --version
```

如果已经安装完整 Xcode，首次启动 Xcode 完成组件安装，然后执行：

```bash
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -runFirstLaunch
sudo xcodebuild -license accept
```

#### 2. 安装 Node.js 和 Rust

安装 Node.js 22 LTS。可以从 [Node.js 官网](https://nodejs.org/)安装，也可以使用 Homebrew：

```bash
brew install node@22
node --version
npm --version
```

通过 `rustup` 安装 Rust stable，并增加 Apple Silicon 目标：

```bash
curl --proto '=https' --tlsv1.2 https://sh.rustup.rs -sSf | sh
source "$HOME/.cargo/env"
rustup default stable
rustup target add aarch64-apple-darwin
rustc --version
cargo --version
```

不需要单独执行 `cargo install tauri-cli`，项目已通过 `@tauri-apps/cli` 固定 Tauri 2 CLI 依赖。

#### 3. 获取项目并安装依赖

```bash
git clone git@github.com:katcuu/GitSyncTools.git
cd GitSyncTools
npm ci
```

构建指定版本时先切换标签，例如：

```bash
git fetch --tags
git checkout v0.3.7
```

日常开发使用 `main` 分支即可。SSH 克隆需要提前配置可用的 GitHub SSH 密钥和 `known_hosts`。

#### 4. 构建前检查

```bash
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

所有命令成功后再生成安装包，避免发布只能启动但核心同步逻辑未通过测试的版本。

#### 5. 构建本地测试版 DMG

不配置 Apple Developer ID 时，可以先构建仅供本机测试的版本：

```bash
npm run tauri -- build \
  --target aarch64-apple-darwin \
  --bundles app,dmg \
  --config '{"bundle":{"createUpdaterArtifacts":false}}'
```

主要产物位于：

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/GitSyncTools.app
src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/GitSyncTools_*.dmg
```

可直接打开 App Bundle 或挂载 DMG 验证：

```bash
open src-tauri/target/aarch64-apple-darwin/release/bundle/macos/GitSyncTools.app
open src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/GitSyncTools_*.dmg
```

未使用 Apple Developer ID 签名的应用在其他 Mac 上可能被 Gatekeeper 阻止，仅适合开发测试。

#### 6. 生成自动更新签名包

Tauri 更新签名与 Apple 代码签名是两套独立机制。更新签名用于证明下载包由 GitSyncTools 发布，项目中的 `createUpdaterArtifacts: true` 会额外生成 macOS 更新包。

将现有 Tauri 更新私钥安全传输到 Mac，并保存在仓库外，例如：

```bash
mkdir -p "$HOME/.tauri"
chmod 700 "$HOME/.tauri"
chmod 600 "$HOME/.tauri/gitsynctools-updater.key"
```

不要提交或复制到项目目录。构建前设置环境变量：

```bash
export TAURI_SIGNING_PRIVATE_KEY="$HOME/.tauri/gitsynctools-updater.key"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
npm run tauri -- build --target aarch64-apple-darwin --bundles app,dmg
unset TAURI_SIGNING_PRIVATE_KEY
unset TAURI_SIGNING_PRIVATE_KEY_PASSWORD
```

除 `.app` 和 `.dmg` 外，还应生成：

```text
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/GitSyncTools.app.tar.gz
src-tauri/target/aarch64-apple-darwin/release/bundle/macos/GitSyncTools.app.tar.gz.sig
```

`.dmg` 用于手动安装；`.app.tar.gz` 和同名 `.sig` 用于应用内自动更新。私钥必须与 `src-tauri/tauri.conf.json` 中现有公钥配对，否则已安装客户端会拒绝更新。

#### 7. Apple Developer ID 签名和公证

向其他用户正式分发时，建议加入 Apple Developer Program，并在“钥匙串访问”中安装 `Developer ID Application` 证书。查找签名身份：

```bash
security find-identity -v -p codesigning
```

设置证书身份：

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Your Name (TEAMID)"
```

使用 Apple ID 公证时，再设置：

```bash
export APPLE_ID="developer@example.com"
export APPLE_PASSWORD="Apple ID 专用密码"
export APPLE_TEAM_ID="你的 Team ID"
```

然后执行与上一节相同的签名构建命令。Tauri 会在打包阶段调用 Apple 工具完成代码签名和公证。不要把 Apple 密码、证书或更新私钥写入仓库；在 CI 中应使用受保护的秘密变量。

构建后可以检查签名和 Gatekeeper 状态：

```bash
codesign --verify --deep --strict --verbose=2 \
  src-tauri/target/aarch64-apple-darwin/release/bundle/macos/GitSyncTools.app
spctl --assess --type execute --verbose=4 \
  src-tauri/target/aarch64-apple-darwin/release/bundle/macos/GitSyncTools.app
```

没有 Apple Developer 账号时仍可生成 DMG 和 Tauri 更新签名，但其他 Mac 首次打开时会出现 Gatekeeper 警告。

#### 8. 发布 macOS 更新文件

GitHub Actions 会将 `.app.tar.gz` 上传到 GitHub Release。对应的版本下载地址格式为：

```text
https://github.com/katcuu/GitSyncTools/releases/download/v0.3.7/GitSyncTools_0.3.7_aarch64.app.tar.gz
```

`latest.json` 需要同时包含 Windows 和 macOS 平台。准备两端的 `.sig` 后，可在安装了 PowerShell 的机器上运行：

```bash
pwsh ./scripts/generate-update-manifest.ps1 \
  -Version "0.3.7" \
  -WindowsUrl "https://github.com/katcuu/GitSyncTools/releases/download/v0.3.7/GitSyncTools_0.3.7_x64-setup.exe" \
  -WindowsSignaturePath "./GitSyncTools_0.3.7_x64-setup.exe.sig" \
  -MacArm64Url "https://github.com/katcuu/GitSyncTools/releases/download/v0.3.7/GitSyncTools_0.3.7_aarch64.app.tar.gz" \
  -MacArm64SignaturePath "./GitSyncTools_0.3.7_aarch64.app.tar.gz.sig" \
  -OutputPath "./latest.json"
```

macOS 平台键固定为 `darwin-aarch64`。上传新的 `latest.json` 后，应在已安装的旧版 Mac 客户端中执行一次“检测更新”验收。

#### 9. 常见问题

- `xcrun`、`clang` 或 SDK 找不到：重新安装 Command Line Tools，并检查 `xcode-select -p`。
- `node`、`cargo` 在终端可用但构建脚本找不到：重新打开终端，确认 `PATH` 包含 Node 和 `$HOME/.cargo/bin`。
- 从 Finder 启动后提示找不到 Git：优先安装 Xcode Command Line Tools 提供的 `/usr/bin/git`；GUI 应用不会读取所有 shell 启动脚本。
- 更新包没有 `.sig`：确认设置了 `TAURI_SIGNING_PRIVATE_KEY`，且 `createUpdaterArtifacts` 保持为 `true`。
- 更新签名无效：确认 Mac 和 Windows 使用同一份 Tauri 更新私钥，不要重新生成另一套密钥。
- 其他 Mac 无法直接打开：本地测试包未做 Apple Developer ID 签名或公证；正式发布必须完成第 7 步。
- 不能在现有 Windows Runner 生成 DMG：macOS 包只能在 Mac 或 macOS Runner 上构建。

## 项目结构

```text
src/                         React/TypeScript 界面与前端更新流程
src-tauri/src/               Rust 命令、Git 操作和同步核心
src-tauri/capabilities/      Tauri 权限配置
src-tauri/windows/           Windows 安装器钩子
scripts/                     发布清单生成脚本
docs/                        发布与维护文档
.github/                     GitHub Actions、Issue 和 Pull Request 模板
```

## 安全边界

- 单文件上限为 50 MB，不支持 Git LFS。
- HTTPS 仓库地址不得嵌入用户名或令牌。
- SSH 使用系统密钥、代理和 `known_hosts`。
- Git 子进程禁用交互式终端，并在 Windows 中隐藏运行。
- 更新包必须通过内置公钥验证；私钥丢失后，已安装客户端将无法信任后续更新。
- 正式公开分发前应配置 Windows 代码签名证书和 Apple Developer ID。

安全问题请按照 [SECURITY.md](SECURITY.md) 私下报告。

## 参与贡献

欢迎提交 Issue 和 Pull Request。开始前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md) 和 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

## 版本记录

变更记录见 [CHANGELOG.md](CHANGELOG.md)。项目遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## 许可证

本项目使用 [MIT License](LICENSE)。

Copyright © 2026 katcoo
