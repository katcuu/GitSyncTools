# 变更记录

本项目的重要变更记录在此文件中。格式参考 [Keep a Changelog](https://keepachangelog.com/zh-CN/1.1.0/)，版本号遵循 [Semantic Versioning](https://semver.org/lang/zh-CN/)。

## [Unreleased]

## [0.3.12] - 2026-07-15

### 修复

- 恢复通过标准 `git add` 暂存同步文件，使 Windows 企业加密/DLP 软件能够按系统既有规则为 `git.exe` 提供解密后的文件内容；适用于所有文件类型。
- 文件经过系统或 Git 处理后，以 Git 索引中的最终 blob 作为同步内容真值，并据此更新清单的大小和 SHA-256，保证 macOS 收到的文件与清单一致。
- 移除 0.3.11 引入的原始字节直接写入 Git 索引逻辑；该逻辑会绕过依赖进程识别的透明解密软件。
- 增加 `staged_content_transformed` 诊断日志，记录工作区与 Git 暂存对象处理前后的大小和哈希。

## [0.3.11] - 2026-07-15

### 修复

- 暂存对象与清单不一致时，从用户明确选择的源文件重新读取原始字节，通过 `git hash-object --stdin` 和 `git update-index --cacheinfo` 直接写入 Git 索引，绕过 AppData 工作区被文件系统过滤、DLP、安全软件或高优先级 clean filter 异步改写的问题。
- 直接写入索引后再次读取 Git blob 并逐字节校验，同时以同一源文件字节修正清单大小和 SHA-256，保证提交、清单与源文件一致。
- 暂存阶段日志增加期望/实际大小、SHA-256 以及 `raw_git_object_success` 恢复结果。

## [0.3.10] - 2026-07-15

### 修复

- 发送端允许用户明确重新选择同名文件或文件夹来替换旧提交中与清单不一致的坏 blob，使 v0.3.7 及更早版本产生的异常提交可以被重新发布修复。
- 识别标准 Git LFS 指针；macOS 安装了 Git LFS 时尝试自动下载并校验原文件，失败时提供明确的重传操作提示。
- 对象校验失败时记录清单期望大小与 SHA-256、Git blob 大小与 SHA-256、对象类型、checkout 文件状态和恢复结果。
- 应用层错误写入诊断日志，不再只记录 Git 命令成功退出而遗漏后续清单校验错误。

## [0.3.9] - 2026-07-15

### 新增

- 增加诊断日志模块，记录更新、同步、删除和 Git 子命令的结果与耗时；日志以两个 10 MB 文件循环存储，总量控制在约 20 MB。
- 连接设置增加“打开日志目录”入口，便于问题发生后直接取得诊断信息。

### 变更

- macOS 更新检测自动读取环境变量和系统 HTTPS、HTTP、SOCKS 代理，并将代理显式传递给 Tauri 更新检测及下载流程。
- 更新器改用操作系统原生 TLS 和系统证书信任，兼容安装了代理根证书的 macOS 环境。
- 仓库同步通过一次 `fetch` 获取远端版本，减少远端发生变化时重复连接。
- 复用已校验的内部 Git 工作区，避免逐文件执行 `git show`、重复哈希及重写；减少重复 Git 配置和文件列表子进程。

## [0.3.8] - 2026-07-15

### 新增

- Windows 和 macOS 系统托盘右键菜单增加“同步”：发送端刷新仓库，接收端无冲突时直接应用更新。

### 修复

- 同步文件强制按二进制存储并禁用用户级 Git clean filter，修复 Office 文档的 Git 对象与清单 SHA-256 不一致导致 macOS 无法更新的问题。
- 提交前校验暂存区 Git 对象与清单，发现外部 attributes/filter 仍在改写文件时阻止上传，避免生成不可用提交。

## [0.3.7] - 2026-07-15

### 新增

- 增加 GitHub Actions CI，在 `main` 和 Pull Request 上执行前端与 Rust 完整检查。
- 增加跨平台 Release 工作流，自动构建 Windows x64、Apple Silicon macOS、Tauri 签名文件和统一 `latest.json`。

### 变更

- 自动更新端点迁移到 GitHub Release，并默认使用系统及环境变量代理设置。
- 项目以不包含旧提交记录的新根提交迁移到 `katcuu/GitSyncTools`。

## [0.3.6] - 2026-07-14

### 新增

- 发送端未选择文件时，同步按钮显示向下图标并用于拉取最新仓库状态；选择文件后切换为向上上传图标。

### 修复

- 上传和删除前默认将内部仓库更新到远端最新提交，远端存在其他设备提交时不再直接停止操作。
- 删除操作携带用户选择文件时看到的仓库版本：目标已被其他设备删除时视为成功，无关文件发生变化时继续删除，同一目标被修改或类型变化时停止并要求重新确认。
- 远端分支被清空或删除后重建内部空仓库，避免文件列表继续显示旧提交内容。

## [0.3.5] - 2026-07-14

### 新增

- 主界面底部显示当前 GitSyncTools 版本，便于确认实际安装版本。
- 接收端分别显示“上次检查”和“上次应用更新”时间；远端无变化时也会更新检查时间。
- 增加 Windows PowerShell 和 Apple Silicon macOS 一键构建脚本，支持指定版本标签并按参数控制 Tauri 自动更新产物。

### 修复

- 启动时清理无需重试的历史瞬时错误，避免旧版 `pathspec 'files' did not match any files` 错误持续显示为当前故障。
- 操作错误提示增加关闭按钮，关闭后同步清除本地持久化错误状态。
- “立即更新”点击后优先刷新忙碌状态并显示当前阶段，改善 Git 网络操作开始前的响应反馈。
- macOS 自动更新构建同时包含 `app` 和 `dmg`，避免只构建 DMG 时跳过更新包并清理 App Bundle。

### 计划

- Windows 11 一级右键菜单扩展。
- Apple Silicon macOS 自动化构建和签名。

## [0.3.4] - 2026-07-14

### 新增

- macOS 接收端支持对仓库文件多选、全选和批量删除，并支持删除提交上传失败后的重试。

### 修复

- 删除仓库文件时，仅暂存实际存在或曾被 Git 跟踪的管理路径，修复空 `files/` 目录触发的 `pathspec 'files' did not match any files`。
- GitLab CI 改用旧版兼容的 `only`、`dependencies` 和作业级 Runner 标签，并使用共享 Runner 的 `intest` 标签。

### 文档

- README 增加 Apple Silicon macOS 的依赖安装、DMG 打包、更新签名、Apple 公证和发布步骤。

## [0.3.3] - 2026-07-14

### 修复

- 自动更新端点改为实际使用的 `http://gitlab.intest.cn` 内部 GitLab Web 地址。
- 发布清单脚本允许为受控内网 GitLab 生成 HTTP 下载地址。

### 安全

- HTTP 仅用于固定内网更新端点，下载完成后仍强制验证 Tauri 数字签名。

## [0.3.2] - 2026-07-14

### 新增

- 仓库文件列表增加多选、全选、取消全选和批量删除操作。
- 支持在一次提交中同时删除 GitSyncTools 同步文件和仓库原有文件。

### 安全

- 删除前统一确认并重新核验远端提交、文件类型和真实仓库路径。
- 禁止通过批量删除修改 `.filesync` 元数据，拒绝路径穿越和链接文件。

## [0.3.1] - 2026-07-14

### 修复

- 允许发送端安全初始化已有提交但尚无 GitSyncTools 清单的仓库。
- 仓库内容区域展示已有但未纳入同步的 Git 文件，并明确标识其状态。
- 接收端遇到未初始化仓库时显示操作提示，不再记录为同步错误。
- 初始设置保存后改为后台下载仓库，不再由完整 `git fetch` 阻塞设置界面。
- 自动更新改善网络、权限和 Release 缺失提示，不再直接显示底层请求错误。
- 更新结果提示增加关闭按钮；“已是最新版”提示也会自动消失。

## [0.3.0] - 2026-07-13

### 新增

- 基于 Tauri 签名更新包和 GitLab Release 的“检测更新、确认、下载、安装”流程。
- GitLab CI、发布清单脚本和中文发布指南。
- 中文开源项目文档、贡献指南、安全策略和 Issue/Merge Request 模板。
- 系统托盘驻留和仓库文件信息展示。

### 优化

- 产品名称统一为 GitSyncTools，发布者统一为 katcoo。
- Git 连接检测增加 20 秒超时和 2 分钟缓存。
- 首次远端获取采用浅层历史，减少不必要下载。
- Windows Git 子进程不再显示命令行窗口。

## [0.1.0] - 2026-07-13

### 新增

- Windows 发送端和 macOS 接收端的基础单向文件同步。
- 文件清单、冲突检测、待推送重试和资源管理器右键入口。

[Unreleased]: https://github.com/katcuu/GitSyncTools/compare/v0.3.12...HEAD
[0.3.12]: https://github.com/katcuu/GitSyncTools/releases/tag/v0.3.12
[0.3.11]: https://github.com/katcuu/GitSyncTools/releases/tag/v0.3.11
[0.3.10]: https://github.com/katcuu/GitSyncTools/releases/tag/v0.3.10
[0.3.9]: https://github.com/katcuu/GitSyncTools/releases/tag/v0.3.9
[0.3.8]: https://github.com/katcuu/GitSyncTools/releases/tag/v0.3.8
[0.3.7]: https://github.com/katcuu/GitSyncTools/releases/tag/v0.3.7
