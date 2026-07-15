# GitSyncTools AI 开发约定

本文件供 Codex、ChatGPT、Claude Code 等编码代理自动读取。开始修改前必须先完整阅读：

1. `docs/PROJECT_CONTEXT.md`：产品边界、架构、关键不变量、历史问题和交接方法。
2. `CONTRIBUTING.md`：提交与测试要求。
3. 涉及打包或升级时再阅读 `docs/RELEASE.md`。

## 不可破坏的约束

- 产品是 Windows 发送、macOS 接收为主的轻量单向同步工具，不擅自扩展为通用双向同步。
- 用户目录中不得生成 `.git`；Git 工作区固定在 Tauri 应用数据目录。
- 不保存明文密码、令牌或更新签名私钥，不把真实用户路径、文件和日志提交进仓库。
- 发布文件必须走标准 `git add`，让 Windows 企业加密/DLP 软件按 `git.exe` 进程规则提供解密内容。
- 同步清单中的文件大小和 SHA-256 必须取自 Git 索引中的最终 blob，不能取代 Git 处理后的源文件原始字节。
- 接收端只覆盖或删除清单管理的路径；不得删除同步目录中的无关文件。
- 继续保留符号链接/junction、路径穿越、损坏清单和单文件 50 MB 限制等安全校验。
- Git、网络和更新错误必须脱敏；关键操作继续记录耗时，日志总量维持约 20 MB 循环存储。
- `com.katcc.lightsync` 是为兼容已有安装和应用数据保留的 identifier，未经迁移设计不要修改。

## 修改后的最低验证

```bash
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

不要仅更新 `package.json` 的版本。发布时必须同步所有版本位置，并由 `vX.Y.Z` 标签触发 GitHub Release 工作流。
