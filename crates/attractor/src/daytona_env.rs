use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use agent::execution_env::{format_lines_numbered, DirEntry, ExecResult, ExecutionEnvironment, GrepOptions};
use async_trait::async_trait;

/// Configuration for a Daytona cloud sandbox execution environment.
pub struct DaytonaConfig {
    /// Docker image to use for the sandbox.
    pub image: String,
    /// Working directory inside the sandbox.
    pub working_directory: String,
}

impl Default for DaytonaConfig {
    fn default() -> Self {
        Self {
            image: "ubuntu:22.04".to_string(),
            working_directory: "/home/daytona/workspace".to_string(),
        }
    }
}

/// Execution environment that runs all operations inside a Daytona cloud sandbox.
pub struct DaytonaExecutionEnvironment {
    config: DaytonaConfig,
    client: daytona_sdk::Client,
    sandbox: tokio::sync::OnceCell<daytona_sdk::Sandbox>,
}

impl DaytonaExecutionEnvironment {
    #[must_use]
    pub fn new(client: daytona_sdk::Client, config: DaytonaConfig) -> Self {
        Self {
            config,
            client,
            sandbox: tokio::sync::OnceCell::new(),
        }
    }

    /// Resolve a path: relative paths are prepended with the working directory.
    fn resolve_path(&self, path: &str) -> String {
        if Path::new(path).is_absolute() {
            path.to_string()
        } else {
            format!("{}/{}", self.config.working_directory, path)
        }
    }

    /// Get the sandbox, returning an error if not yet initialized.
    fn sandbox(&self) -> Result<&daytona_sdk::Sandbox, String> {
        self.sandbox
            .get()
            .ok_or_else(|| "Daytona sandbox not initialized — call initialize() first".to_string())
    }
}

/// Detect the git remote URL and current branch from a local repository.
///
/// Uses `git2` to discover the repo at `path`, reads the `origin` remote URL
/// and the HEAD branch name.
pub fn detect_repo_info(path: &Path) -> Result<(String, Option<String>), String> {
    let repo = git2::Repository::discover(path)
        .map_err(|e| format!("Failed to discover git repo at {}: {e}", path.display()))?;

    let url = repo
        .find_remote("origin")
        .map_err(|e| format!("Failed to find 'origin' remote: {e}"))?
        .url()
        .ok_or_else(|| "origin remote URL is not valid UTF-8".to_string())?
        .to_string();

    let branch = repo
        .head()
        .ok()
        .and_then(|head| head.shorthand().map(String::from));

    Ok((url, branch))
}

/// Get a GitHub authentication token via `gh auth token`.
pub fn get_gh_token() -> Result<String, String> {
    let output = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .map_err(|e| format!("Failed to run 'gh auth token': {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "gh auth token failed (exit code {}): {stderr}",
            output.status.code().unwrap_or(-1)
        ));
    }

    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err("gh auth token returned empty string".to_string());
    }
    Ok(token)
}

#[async_trait]
impl ExecutionEnvironment for DaytonaExecutionEnvironment {
    async fn initialize(&self) -> Result<(), String> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

        let params = daytona_sdk::CreateParams::Image(daytona_sdk::ImageParams {
            base: daytona_sdk::SandboxBaseParams {
                ephemeral: Some(true),
                ..Default::default()
            },
            image: daytona_sdk::ImageSource::Name(self.config.image.clone()),
            resources: None,
        });

        let sandbox = self
            .client
            .create(params, daytona_sdk::CreateSandboxOptions::default())
            .await
            .map_err(|e| format!("Failed to create Daytona sandbox: {e}"))?;

        // Clone the repo into the sandbox
        match detect_repo_info(&cwd) {
            Ok((url, branch)) => {
                let token = get_gh_token()
                    .map_err(|e| format!("Failed to get GitHub token for Daytona clone: {e}"))?;

                let git_svc = sandbox
                    .git()
                    .await
                    .map_err(|e| format!("Failed to get Daytona git service: {e}"))?;

                git_svc
                    .clone(
                        &url,
                        &self.config.working_directory,
                        daytona_sdk::GitCloneOptions {
                            branch,
                            username: Some("x-access-token".to_string()),
                            password: Some(token),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| format!("Failed to clone repo into Daytona sandbox: {e}"))?;
            }
            Err(e) => {
                eprintln!("Warning: could not detect git repo for Daytona clone: {e}");
                // Create working directory even without a repo
                let fs_svc = sandbox
                    .fs()
                    .await
                    .map_err(|e| format!("Failed to get Daytona fs service: {e}"))?;
                fs_svc
                    .create_folder(&self.config.working_directory, None)
                    .await
                    .map_err(|e| format!("Failed to create working directory: {e}"))?;
            }
        }

        self.sandbox
            .set(sandbox)
            .map_err(|_| "Daytona sandbox already initialized".to_string())?;

        Ok(())
    }

    async fn cleanup(&self) -> Result<(), String> {
        if let Some(sandbox) = self.sandbox.get() {
            sandbox
                .delete()
                .await
                .map_err(|e| format!("Failed to delete Daytona sandbox: {e}"))?;
        }
        Ok(())
    }

    fn working_directory(&self) -> &str {
        &self.config.working_directory
    }

    fn platform(&self) -> &str {
        "linux"
    }

    fn os_version(&self) -> String {
        "Linux (Daytona)".to_string()
    }

    async fn read_file(
        &self,
        path: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<String, String> {
        let sandbox = self.sandbox()?;
        let resolved = self.resolve_path(path);

        let fs_svc = sandbox
            .fs()
            .await
            .map_err(|e| format!("Failed to get fs service: {e}"))?;

        let bytes = fs_svc
            .download_file(&resolved)
            .await
            .map_err(|e| format!("Failed to read file {resolved}: {e}"))?;

        let content =
            String::from_utf8(bytes).map_err(|e| format!("File is not valid UTF-8: {e}"))?;

        Ok(format_lines_numbered(&content, offset, limit))
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<(), String> {
        let sandbox = self.sandbox()?;
        let resolved = self.resolve_path(path);

        // Ensure parent directory exists
        if let Some(parent) = Path::new(&resolved).parent() {
            let parent_str = parent.to_string_lossy();
            if parent_str != "/" {
                let fs_svc = sandbox
                    .fs()
                    .await
                    .map_err(|e| format!("Failed to get fs service: {e}"))?;
                let _ = fs_svc.create_folder(&parent_str, None).await;
            }
        }

        let fs_svc = sandbox
            .fs()
            .await
            .map_err(|e| format!("Failed to get fs service: {e}"))?;

        fs_svc
            .upload_file_bytes(&resolved, content.as_bytes())
            .await
            .map_err(|e| format!("Failed to write file {resolved}: {e}"))?;

        Ok(())
    }

    async fn delete_file(&self, path: &str) -> Result<(), String> {
        let sandbox = self.sandbox()?;
        let resolved = self.resolve_path(path);

        let fs_svc = sandbox
            .fs()
            .await
            .map_err(|e| format!("Failed to get fs service: {e}"))?;

        fs_svc
            .delete_file(&resolved, false)
            .await
            .map_err(|e| format!("Failed to delete file {resolved}: {e}"))?;

        Ok(())
    }

    async fn file_exists(&self, path: &str) -> Result<bool, String> {
        let sandbox = self.sandbox()?;
        let resolved = self.resolve_path(path);

        let fs_svc = sandbox
            .fs()
            .await
            .map_err(|e| format!("Failed to get fs service: {e}"))?;

        match fs_svc.get_file_info(&resolved).await {
            Ok(_) => Ok(true),
            Err(daytona_sdk::DaytonaError::NotFound { .. }) => Ok(false),
            Err(e) => Err(format!("Failed to check file existence {resolved}: {e}")),
        }
    }

    async fn list_directory(
        &self,
        path: &str,
        _depth: Option<usize>,
    ) -> Result<Vec<DirEntry>, String> {
        let sandbox = self.sandbox()?;
        let resolved = self.resolve_path(path);

        let fs_svc = sandbox
            .fs()
            .await
            .map_err(|e| format!("Failed to get fs service: {e}"))?;

        let files = fs_svc
            .list_files(&resolved)
            .await
            .map_err(|e| format!("Failed to list directory {resolved}: {e}"))?;

        Ok(files
            .into_iter()
            .map(|f| DirEntry {
                name: f.name,
                is_dir: f.is_dir,
                size: if f.size > 0 {
                    Some(f.size as u64)
                } else {
                    None
                },
            })
            .collect())
    }

    async fn exec_command(
        &self,
        command: &str,
        timeout_ms: u64,
        working_dir: Option<&str>,
        _env_vars: Option<&HashMap<String, String>>,
        _cancel_token: Option<tokio_util::sync::CancellationToken>,
    ) -> Result<ExecResult, String> {
        let sandbox = self.sandbox()?;
        let start = Instant::now();

        let cwd = working_dir
            .map(|d| self.resolve_path(d))
            .unwrap_or_else(|| self.config.working_directory.clone());

        let process_svc = sandbox
            .process()
            .await
            .map_err(|e| format!("Failed to get process service: {e}"))?;

        let options = daytona_sdk::ExecuteCommandOptions {
            cwd: Some(cwd),
            timeout: Some(std::time::Duration::from_millis(timeout_ms)),
            ..Default::default()
        };

        let result = process_svc
            .execute_command(command, options)
            .await
            .map_err(|e| format!("Failed to execute command: {e}"))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        // The Daytona SDK returns combined output in `result` field.
        // Separate stderr isn't available in the simple execute_command API.
        Ok(ExecResult {
            stdout: result.result.clone(),
            stderr: String::new(),
            exit_code: result.exit_code,
            timed_out: false,
            duration_ms,
        })
    }

    async fn grep(
        &self,
        pattern: &str,
        path: &str,
        options: &GrepOptions,
    ) -> Result<Vec<String>, String> {
        let resolved = self.resolve_path(path);

        // Build rg command (same approach as Docker env)
        let mut cmd = "rg --line-number --no-heading".to_string();
        if options.case_insensitive {
            cmd.push_str(" -i");
        }
        if let Some(ref glob_filter) = options.glob_filter {
            cmd.push_str(&format!(" --glob '{glob_filter}'"));
        }
        if let Some(max) = options.max_results {
            cmd.push_str(&format!(" --max-count {max}"));
        }
        cmd.push_str(&format!(" -- '{}' '{}'", pattern.replace('\'', "'\\''"), resolved));

        let result = self.exec_command(&cmd, 30_000, None, None, None).await?;

        if result.exit_code == 1 {
            // rg exits 1 for no matches
            return Ok(Vec::new());
        }
        if result.exit_code != 0 {
            return Err(format!("grep failed (exit {}): {}", result.exit_code, result.stderr));
        }

        Ok(result
            .stdout
            .lines()
            .map(String::from)
            .collect())
    }

    async fn glob(&self, pattern: &str, path: Option<&str>) -> Result<Vec<String>, String> {
        let base = path
            .map(|p| self.resolve_path(p))
            .unwrap_or_else(|| self.config.working_directory.clone());

        let cmd = format!(
            "find '{}' -name '{}' -type f | sort",
            base.replace('\'', "'\\''"),
            pattern.replace('\'', "'\\''"),
        );

        let result = self.exec_command(&cmd, 30_000, None, None, None).await?;

        if result.exit_code != 0 {
            return Err(format!("glob failed (exit {}): {}", result.exit_code, result.stderr));
        }

        Ok(result
            .stdout
            .lines()
            .filter(|l| !l.is_empty())
            .map(String::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daytona_config_defaults() {
        let config = DaytonaConfig::default();
        assert_eq!(config.image, "ubuntu:22.04");
        assert_eq!(config.working_directory, "/home/daytona/workspace");
    }

    #[test]
    fn detect_git_remote_from_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        repo.remote("origin", "https://github.com/org/repo.git")
            .unwrap();

        let (url, _branch) = detect_repo_info(dir.path()).unwrap();
        assert_eq!(url, "https://github.com/org/repo.git");
    }

    #[test]
    fn detect_git_branch_from_repo() {
        let dir = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create an initial commit so HEAD points to a branch
        let sig = git2::Signature::now("Test", "test@test.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        repo.remote("origin", "https://github.com/org/repo.git")
            .unwrap();

        let (_, branch) = detect_repo_info(dir.path()).unwrap();
        // git init creates "master" or "main" depending on git config
        assert!(branch.is_some());
    }

    #[test]
    #[ignore] // requires `gh` CLI installed and authenticated
    fn gh_auth_token_returns_nonempty_string() {
        let token = get_gh_token().unwrap();
        assert!(!token.is_empty());
    }
}
