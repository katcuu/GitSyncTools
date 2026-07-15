use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::model::AppConfig;

#[derive(Debug, Clone)]
pub struct GitRepository {
    root: PathBuf,
    remote_url: String,
    branch: String,
}

#[derive(Debug, Clone)]
pub struct RemoteInspection {
    pub head: Option<String>,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTreeFile {
    pub path: String,
    pub object_id: String,
    pub size: u64,
}

impl GitRepository {
    pub fn new(root: PathBuf, config: &AppConfig) -> Result<Self, String> {
        validate_branch(&config.branch)?;
        Ok(Self {
            root,
            remote_url: config.repository_url.clone(),
            branch: config.branch.clone(),
        })
    }

    pub fn inspect_connection(remote_url: &str, branch: &str) -> Result<RemoteInspection, String> {
        let _timer = crate::diagnostics::OperationTimer::new("git_remote_inspection");
        if remote_url.trim().is_empty() {
            return Err("仓库地址不能为空".into());
        }
        validate_branch(branch)?;
        let reference = format!("refs/heads/{branch}");
        let output = run_git_with_timeout(
            None,
            ["ls-remote", "--heads", remote_url, reference.as_str()],
            Duration::from_secs(20),
        )?;
        if !output.status.success() {
            return Err(command_error(&output, remote_url));
        }
        let output = String::from_utf8_lossy(&output.stdout);
        let head = output
            .lines()
            .find_map(|line| line.split_whitespace().next())
            .map(ToOwned::to_owned);
        let message = if head.is_some() {
            "连接成功".into()
        } else {
            "连接成功，目标分支尚不存在".into()
        };
        Ok(RemoteInspection { head, message })
    }

    pub fn ensure(&self) -> Result<(), String> {
        if self.root.join(".git").is_dir() {
            let configured = self.run(["remote", "get-url", "origin"])?;
            if configured.trim() != self.remote_url {
                return Err("内部仓库与当前设置不一致，请重新保存连接设置".into());
            }
            return Ok(());
        }

        if self.root.exists() {
            fs::remove_dir_all(&self.root)
                .map_err(|error| format!("无法清理未完成的内部仓库：{error}"))?;
        }
        fs::create_dir_all(&self.root).map_err(|error| format!("无法创建内部仓库：{error}"))?;
        self.run(["init", "-b", self.branch.as_str()])?;
        self.run(["remote", "add", "origin", self.remote_url.as_str()])?;
        Ok(())
    }

    pub fn remote_head(&self) -> Result<Option<String>, String> {
        Ok(Self::inspect_connection(&self.remote_url, &self.branch)?.head)
    }

    pub fn head(&self) -> Result<Option<String>, String> {
        let output = self.run_raw(["rev-parse", "--verify", "HEAD"])?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            ))
        } else {
            Ok(None)
        }
    }

    pub fn checkout_remote(&self, remote_head: &str) -> Result<(), String> {
        self.run([
            "fetch",
            "--depth=2",
            "--no-tags",
            "origin",
            self.branch.as_str(),
        ])?;
        let fetched = self.run(["rev-parse", "FETCH_HEAD"])?;
        if fetched.trim() != remote_head {
            return Err("远端在更新过程中发生变化，请重试".into());
        }
        self.run(["reset", "--hard", "FETCH_HEAD"])?;
        self.run(["clean", "-fd", "--", "files", ".filesync"])?;
        Ok(())
    }

    pub fn sync_with_remote(&self) -> Result<Option<String>, String> {
        let remote = self.fetch_remote_head()?;
        match remote.as_deref() {
            Some(_) => {
                self.run(["reset", "--hard", "FETCH_HEAD"])?;
                self.run(["clean", "-fd", "--", "files", ".filesync"])?;
            }
            None => {
                if self.head()?.is_some() {
                    fs::remove_dir_all(&self.root)
                        .map_err(|error| format!("无法重建空的内部仓库：{error}"))?;
                    self.ensure()?;
                }
            }
        }
        Ok(remote)
    }

    fn fetch_remote_head(&self) -> Result<Option<String>, String> {
        let output = self.run_raw([
            "fetch",
            "--depth=2",
            "--no-tags",
            "origin",
            self.branch.as_str(),
        ])?;
        if output.status.success() {
            return self.run(["rev-parse", "FETCH_HEAD"]).map(Some);
        }
        if self.remote_head()?.is_none() {
            return Ok(None);
        }
        Err(command_error(&output, &self.remote_url))
    }

    pub fn stage_all(&self) -> Result<(), String> {
        let mut arguments = collect_args(["add", "--all", "--"]);
        let tracked = self.run(["ls-files", "--", "files", ".filesync", ".gitattributes"])?;
        for path in ["files", ".filesync", ".gitattributes"] {
            let tracked_path = tracked
                .lines()
                .any(|entry| entry == path || entry.starts_with(&format!("{path}/")));
            if self.root.join(path).exists() || tracked_path {
                arguments.push(OsString::from(path));
            }
        }
        if arguments.len() == 3 {
            return Ok(());
        }
        let output = self.run_raw(arguments)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_error(&output, &self.remote_url))
        }
    }

    pub fn stage_paths(&self, paths: &[String]) -> Result<(), String> {
        if paths.is_empty() {
            return Ok(());
        }
        let mut arguments = collect_args(["add", "--all", "--"]);
        arguments.extend(
            paths
                .iter()
                .map(|path| OsString::from(format!(":(literal){path}"))),
        );
        let output = self.run_raw(arguments)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_error(&output, &self.remote_url))
        }
    }

    pub fn has_staged_changes(&self) -> Result<bool, String> {
        let output = self.run_raw(["diff", "--cached", "--quiet"])?;
        match output.status.code() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(command_error(&output, &self.remote_url)),
        }
    }

    pub fn commit(&self, message: &str) -> Result<String, String> {
        self.run([
            "-c",
            "user.name=katcoo",
            "-c",
            "user.email=katcoo@localhost",
            "commit",
            "-m",
            message,
        ])?;
        self.head()?
            .ok_or_else(|| "提交完成后无法读取版本号".into())
    }

    pub fn push_head(&self) -> Result<(), String> {
        let refspec = format!("HEAD:refs/heads/{}", self.branch);
        self.run(["push", "origin", refspec.as_str()])?;
        Ok(())
    }

    pub fn parent_of(&self, commit: &str) -> Result<Option<String>, String> {
        let reference = format!("{commit}^");
        let output = self.run_raw(["rev-parse", "--verify", reference.as_str()])?;
        if output.status.success() {
            Ok(Some(
                String::from_utf8_lossy(&output.stdout).trim().to_owned(),
            ))
        } else {
            Ok(None)
        }
    }

    pub fn read_blob(&self, commit: &str, path: &str) -> Result<Vec<u8>, String> {
        let object = format!("{commit}:{path}");
        let output = self.run_raw(["show", "--no-textconv", object.as_str()])?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(command_error(&output, &self.remote_url))
        }
    }

    pub fn read_staged_blob(&self, path: &str) -> Result<Vec<u8>, String> {
        let object = format!(":{path}");
        let output = self.run_raw(["show", "--no-textconv", object.as_str()])?;
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(command_error(&output, &self.remote_url))
        }
    }

    pub fn list_tree_files(&self, commit: &str) -> Result<Vec<GitTreeFile>, String> {
        let output = self.run_raw(["ls-tree", "-r", "-l", "-z", "--full-tree", commit, "--"])?;
        if !output.status.success() {
            return Err(command_error(&output, &self.remote_url));
        }
        parse_tree_files(&output.stdout)
    }

    pub fn commit_date(&self, commit: &str) -> Result<String, String> {
        self.run(["show", "-s", "--format=%cI", commit])
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn run<I, S>(&self, args: I) -> Result<String, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = self.run_raw(args)?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            Err(command_error(&output, &self.remote_url))
        }
    }

    fn run_raw<I, S>(&self, args: I) -> Result<Output, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        run_git_raw(Some(&self.root), args)
    }
}

fn parse_tree_files(output: &[u8]) -> Result<Vec<GitTreeFile>, String> {
    let mut files = Vec::new();
    for record in output
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
    {
        let Some(tab) = record.iter().position(|byte| *byte == b'\t') else {
            return Err("无法解析 Git 文件列表".into());
        };
        let metadata = String::from_utf8_lossy(&record[..tab]);
        let mut fields = metadata.split_whitespace();
        let _mode = fields.next();
        let kind = fields.next();
        let object_id = fields.next();
        let size = fields.next();
        if kind != Some("blob") {
            continue;
        }
        let size = size
            .and_then(|value| value.parse::<u64>().ok())
            .ok_or_else(|| "无法解析 Git 文件大小".to_string())?;
        let object_id = object_id.ok_or_else(|| "无法解析 Git 文件版本".to_string())?;
        files.push(GitTreeFile {
            path: String::from_utf8_lossy(&record[tab + 1..]).into_owned(),
            object_id: object_id.to_owned(),
            size,
        });
    }
    Ok(files)
}

fn run_git_raw<I, S>(cwd: Option<&Path>, args: I) -> Result<Output, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = collect_args(args);
    let command_name = git_operation(&args);
    let started = Instant::now();
    let result = git_command(cwd, &args).output().map_err(git_launch_error);
    match &result {
        Ok(output) => log::info!(
            "operation=git command={} duration_ms={} exit_code={}",
            command_name,
            started.elapsed().as_millis(),
            output.status.code().unwrap_or(-1)
        ),
        Err(_) => log::warn!(
            "operation=git command={} duration_ms={} result=launch_error",
            command_name,
            started.elapsed().as_millis()
        ),
    }
    result
}

fn run_git_with_timeout<I, S>(
    cwd: Option<&Path>,
    args: I,
    timeout: Duration,
) -> Result<Output, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args = collect_args(args);
    let command_name = git_operation(&args);
    let mut command = git_command(cwd, &args);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(git_launch_error)?;
    let started = Instant::now();
    loop {
        match child.try_wait().map_err(git_launch_error)? {
            Some(_) => {
                let output = child.wait_with_output().map_err(git_launch_error)?;
                log::info!(
                    "operation=git command={} duration_ms={} exit_code={}",
                    command_name,
                    started.elapsed().as_millis(),
                    output.status.code().unwrap_or(-1)
                );
                return Ok(output);
            }
            None if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!(
                    "连接检测超过 {} 秒，请检查网络、代理或 Git 认证配置",
                    timeout.as_secs()
                ));
            }
            None => thread::sleep(Duration::from_millis(50)),
        }
    }
}

fn git_operation(args: &[OsString]) -> String {
    args.iter()
        .map(|value| value.to_string_lossy())
        .find(|value| !value.starts_with('-') && !value.contains('='))
        .map(|value| value.into_owned())
        .unwrap_or_else(|| "unknown".into())
}

fn collect_args<I, S>(args: I) -> Vec<OsString>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    args.into_iter()
        .map(|value| value.as_ref().to_os_string())
        .collect()
}

fn git_command(cwd: Option<&Path>, args: &[OsString]) -> Command {
    let mut command = Command::new("git");
    command
        .args(args)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never")
        .env("SSH_ASKPASS_REQUIRE", "never");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }
    command
}

fn git_launch_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        "未找到 Git，请先安装 Git 并重新启动 GitSyncTools".into()
    } else {
        format!("无法运行 Git：{error}")
    }
}

fn command_error(output: &Output, secret: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let text = if stderr.trim().is_empty() {
        stdout.trim()
    } else {
        stderr.trim()
    };
    let sanitized = sanitize(text, secret);
    if sanitized.is_empty() {
        format!(
            "Git 操作失败，退出码 {}",
            output.status.code().unwrap_or(-1)
        )
    } else {
        format!("Git 操作失败：{sanitized}")
    }
}

fn sanitize(value: &str, secret: &str) -> String {
    let mut sanitized = value.replace(['\r', '\n'], " ");
    if !secret.is_empty() {
        sanitized = sanitized.replace(secret, "<repository>");
    }
    if let Some(scheme) = sanitized.find("://") {
        let credentials_start = scheme + 3;
        if let Some(relative_at) = sanitized[credentials_start..].find('@') {
            let at = credentials_start + relative_at;
            sanitized.replace_range(credentials_start..=at, "<credentials>@");
        }
    }
    sanitized.chars().take(800).collect()
}

pub fn validate_branch(branch: &str) -> Result<(), String> {
    let invalid = branch.is_empty()
        || branch.starts_with('-')
        || branch.starts_with('/')
        || branch.ends_with('/')
        || branch.ends_with('.')
        || branch.ends_with(".lock")
        || branch.contains("..")
        || branch.contains("@{")
        || branch.contains([' ', '~', '^', ':', '?', '*', '[', '\\'])
        || branch.chars().any(char::is_control);
    if invalid {
        Err("分支名称无效".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn validates_branch_names() {
        assert!(validate_branch("main").is_ok());
        assert!(validate_branch("sync/files").is_ok());
        assert!(validate_branch("../main").is_err());
        assert!(validate_branch("-danger").is_err());
        assert!(validate_branch("feature lock").is_err());
    }

    #[test]
    fn removes_credentials_from_errors() {
        let text = sanitize(
            "fatal: https://user:token@example.com/private.git failed",
            "",
        );
        assert!(!text.contains("token"));
        assert!(text.contains("<credentials>@example.com"));
    }

    #[test]
    fn parses_null_delimited_tree_files() {
        let output = b"100644 blob abc123 12\tdocs/read me.txt\0\
100644 blob def456 5\tunicode/\xE6\x96\x87\xE4\xBB\xB6.txt\0";
        let files = parse_tree_files(output).unwrap();
        assert_eq!(
            files,
            vec![
                GitTreeFile {
                    path: "docs/read me.txt".into(),
                    object_id: "abc123".into(),
                    size: 12,
                },
                GitTreeFile {
                    path: "unicode/文件.txt".into(),
                    object_id: "def456".into(),
                    size: 5,
                },
            ]
        );
    }

    #[test]
    fn stages_metadata_when_files_directory_does_not_exist() {
        let temp = tempdir().unwrap();
        let repository = GitRepository {
            root: temp.path().to_path_buf(),
            remote_url: String::new(),
            branch: "main".into(),
        };
        repository.run(["init", "-b", "main"]).unwrap();
        fs::create_dir_all(temp.path().join(".filesync")).unwrap();
        fs::write(temp.path().join(".filesync/manifest.json"), b"{}").unwrap();
        fs::write(temp.path().join(".gitattributes"), b"* text=auto\n").unwrap();

        repository.stage_all().unwrap();

        let staged = repository.run(["diff", "--cached", "--name-only"]).unwrap();
        assert!(staged.lines().any(|path| path == ".filesync/manifest.json"));
        assert!(staged.lines().any(|path| path == ".gitattributes"));
    }
}
