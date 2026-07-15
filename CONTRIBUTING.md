# 贡献指南

感谢参与 GitSyncTools。为便于审查和发布，请遵循以下约定。

## 开始之前

1. 搜索现有 Issue，避免重复问题。
2. Bug 请提供系统版本、Git 版本、复现步骤、预期结果和已脱敏日志。
3. 较大的功能变更请先创建 Issue 讨论范围和兼容性。
4. 安全问题不要创建公开 Issue，请按照 [SECURITY.md](SECURITY.md) 报告。

## 分支与提交

- 从 `main` 创建短期分支，例如 `feature/updater`、`fix/git-timeout`。
- 每个提交只解决一个清晰问题。
- 提交信息使用中文或英文祈使句，例如 `feat: 增加更新进度提示`。
- 推荐类型：`feat`、`fix`、`docs`、`test`、`refactor`、`build`、`chore`。
- 不得提交仓库令牌、更新签名私钥、个人路径或真实用户文件。

## 本地检查

提交 Pull Request 前至少运行：

```bash
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

涉及同步逻辑时，应补充 Rust 测试；涉及界面或路径处理时，应补充前端测试。

## Pull Request

- 描述问题、实现方案、风险和验证结果。
- 关联对应 Issue。
- UI 变更附截图。
- 不混入无关格式化或重构。
- 确保 CI 全部通过，并处理审查意见。

提交贡献即表示你有权提供相关代码，并同意其按项目的 MIT License 发布。
