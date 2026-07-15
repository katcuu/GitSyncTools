# GitSyncTools 发布与自动更新指南

项目使用 GitHub Actions 构建 Windows x64 和 Apple Silicon macOS 安装包，并通过 GitHub Release 提供 Tauri 签名自动更新。

## 更新链路

1. 客户端请求最新 Release 的 `latest.json`。
2. Tauri 比较远端版本与当前版本。
3. 用户确认后，客户端下载对应平台更新包。
4. Tauri 使用应用内置公钥验证更新签名。
5. 校验成功后安装更新并重新启动。

客户端端点：

```text
https://github.com/katcuu/GitSyncTools/releases/latest/download/latest.json
```

更新客户端默认使用系统代理以及 `HTTP_PROXY`、`HTTPS_PROXY`、`NO_PROXY` 环境变量。GitHub Release 必须允许客户端匿名下载；不要在桌面应用中硬编码 GitHub Token。

## GitHub Actions Secrets

进入仓库的 `Settings > Secrets and variables > Actions`，创建：

| Secret | 要求 |
| --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | 与 `src-tauri/tauri.conf.json` 中公钥配对的私钥完整内容，必填 |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | 私钥密码；当前私钥无密码时可不创建或留空 |

私钥是已安装客户端的长期信任根，不能提交到仓库、Release、Issue、Actions 日志或普通变量。

## CI 与发布工作流

- `.github/workflows/ci.yml`：对 `main` 分支和 Pull Request 执行前端测试、构建、Rust 格式检查、Clippy 和 Rust 测试。
- `.github/workflows/release.yml`：收到 `v*` 标签后构建两个平台，生成签名文件和统一 `latest.json`，最后创建 GitHub Release。

发布工作流产物：

```text
GitSyncTools_X.Y.Z_x64-setup.exe
GitSyncTools_X.Y.Z_x64-setup.exe.sig
GitSyncTools_X.Y.Z_aarch64.dmg
GitSyncTools_X.Y.Z_aarch64.app.tar.gz
GitSyncTools_X.Y.Z_aarch64.app.tar.gz.sig
latest.json
```

## 发布步骤

1. 更新 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 中的版本号。
2. 在 `CHANGELOG.md` 中补充版本记录。
3. 本地运行完整检查：

   ```bash
   npm ci
   npm test
   npm run build
   cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
   cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
   cargo test --manifest-path src-tauri/Cargo.toml
   ```

4. 提交版本变更并推送 `main`。
5. 确认 GitHub Actions 的 `CI` 工作流成功。
6. 创建并推送版本标签：

   ```bash
   git tag -a vX.Y.Z -m "GitSyncTools vX.Y.Z"
   git push origin vX.Y.Z
   ```

7. 等待 `Release` 工作流完成，检查 Release 中六个资产均已生成。
8. 在旧版本 Windows 和 macOS 客户端中点击“检测更新”完成验收。

## 手动生成 latest.json

GitHub Actions 会自动执行以下等价操作：

```powershell
.\scripts\generate-update-manifest.ps1 `
  -Version "0.3.7" `
  -WindowsUrl "https://github.com/katcuu/GitSyncTools/releases/download/v0.3.7/GitSyncTools_0.3.7_x64-setup.exe" `
  -WindowsSignaturePath ".\GitSyncTools_0.3.7_x64-setup.exe.sig" `
  -MacArm64Url "https://github.com/katcuu/GitSyncTools/releases/download/v0.3.7/GitSyncTools_0.3.7_aarch64.app.tar.gz" `
  -MacArm64SignaturePath ".\GitSyncTools_0.3.7_aarch64.app.tar.gz.sig" `
  -OutputPath ".\latest.json"
```

平台键固定为 `windows-x86_64` 和 `darwin-aarch64`。

## 常见失败

- 工作流提示缺少 `TAURI_SIGNING_PRIVATE_KEY`：在 GitHub Actions Secrets 中保存现有 Tauri 私钥后重新运行。
- 版本标签不匹配：确保标签去掉 `v` 后与 `package.json` 版本一致。
- 客户端提示签名无效：Windows 和 macOS 必须使用同一份现有私钥，不能重新生成另一套密钥。
- 客户端无法访问 GitHub：检查系统代理或设置 `HTTPS_PROXY`；确认 Release 和仓库允许下载。
- macOS 构建无法直接安装：Tauri 更新签名不等同于 Apple Developer ID 签名和公证。
