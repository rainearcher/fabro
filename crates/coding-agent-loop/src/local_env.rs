use crate::execution_env::{DirEntry, ExecResult, ExecutionEnvironment, GrepOptions};
use async_trait::async_trait;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::io::AsyncReadExt;
use tokio::process::Command;

pub struct LocalExecutionEnvironment {
    working_directory: PathBuf,
}

impl LocalExecutionEnvironment {
    #[must_use]
    pub const fn new(working_directory: PathBuf) -> Self {
        Self { working_directory }
    }

    fn format_line_numbered(content: &str) -> String {
        use std::fmt::Write;
        let lines: Vec<&str> = content.lines().collect();
        let width = lines.len().to_string().len().max(1);
        let mut result = String::new();
        let mut line_num = 1;
        for line in &lines {
            let _ = writeln!(result, "{line_num:>width$} | {line}");
            line_num += 1;
        }
        result
    }

    fn should_filter_env_var(key: &str) -> bool {
        let lower = key.to_lowercase();
        lower.ends_with("_api_key")
            || lower.ends_with("_secret")
            || lower.ends_with("_token")
            || lower.ends_with("_password")
            || lower.ends_with("_credential")
    }

    fn resolve_path(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.working_directory.join(p)
        }
    }
}

#[async_trait]
impl ExecutionEnvironment for LocalExecutionEnvironment {
    async fn read_file(&self, path: &str) -> Result<String, String> {
        let full_path = self.resolve_path(path);
        let content = tokio::fs::read_to_string(&full_path)
            .await
            .map_err(|e| format!("Failed to read {}: {e}", full_path.display()))?;
        Ok(Self::format_line_numbered(&content))
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<(), String> {
        let full_path = self.resolve_path(path);
        if let Some(parent) = full_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("Failed to create parent dirs: {e}"))?;
        }
        tokio::fs::write(&full_path, content)
            .await
            .map_err(|e| format!("Failed to write {}: {e}", full_path.display()))
    }

    async fn file_exists(&self, path: &str) -> Result<bool, String> {
        let full_path = self.resolve_path(path);
        Ok(full_path.exists())
    }

    async fn list_directory(&self, path: &str) -> Result<Vec<DirEntry>, String> {
        let full_path = self.resolve_path(path);
        let mut entries = Vec::new();
        let mut read_dir = tokio::fs::read_dir(&full_path)
            .await
            .map_err(|e| format!("Failed to read directory {}: {e}", full_path.display()))?;

        while let Some(entry) = read_dir
            .next_entry()
            .await
            .map_err(|e| format!("Failed to read entry: {e}"))?
        {
            let metadata = entry
                .metadata()
                .await
                .map_err(|e| format!("Failed to read metadata: {e}"))?;
            entries.push(DirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: metadata.is_dir(),
                size: if metadata.is_file() {
                    Some(metadata.len())
                } else {
                    None
                },
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(entries)
    }

    async fn exec_command(
        &self,
        command: &str,
        args: &[String],
        timeout_ms: u64,
    ) -> Result<ExecResult, String> {
        let start = Instant::now();

        let filtered_env: Vec<(String, String)> = std::env::vars()
            .filter(|(key, _)| !Self::should_filter_env_var(key))
            .collect();

        let mut cmd = Command::new(command);
        cmd.args(args)
            .current_dir(&self.working_directory)
            .env_clear()
            .envs(filtered_env)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn {command}: {e}"))?;

        let timeout_duration = std::time::Duration::from_millis(timeout_ms);

        let (timed_out, exit_code) =
            if let Ok(status_result) = tokio::time::timeout(timeout_duration, child.wait()).await {
                let status =
                    status_result.map_err(|e| format!("Failed to wait for process: {e}"))?;
                (false, status.code().unwrap_or(-1))
            } else {
                let _ = child.kill().await;
                let _ = child.wait().await;
                (true, -1)
            };

        let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);

        let mut stdout_str = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            let _ = stdout.read_to_string(&mut stdout_str).await;
        }
        let mut stderr_str = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut stderr_str).await;
        }

        Ok(ExecResult {
            stdout: stdout_str,
            stderr: stderr_str,
            exit_code,
            timed_out,
            duration_ms,
        })
    }

    async fn grep(
        &self,
        pattern: &str,
        path: &str,
        options: &GrepOptions,
    ) -> Result<Vec<String>, String> {
        let full_path = self.resolve_path(path);

        let mut args = vec!["-rn".to_string()];
        if options.case_insensitive {
            args.push("-i".into());
        }
        if let Some(ref glob_filter) = options.glob_filter {
            args.push("--include".into());
            args.push(glob_filter.clone());
        }
        if let Some(max) = options.max_results {
            args.push("-m".into());
            args.push(max.to_string());
        }
        args.push(pattern.into());
        args.push(full_path.to_string_lossy().into_owned());

        let output = std::process::Command::new("grep")
            .args(&args)
            .output()
            .map_err(|e| format!("Failed to run grep: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let results: Vec<String> = stdout.lines().map(String::from).filter(|l| !l.is_empty()).collect();
        Ok(results)
    }

    async fn glob(&self, pattern: &str) -> Result<Vec<String>, String> {
        // Use find + fnmatch-style pattern via shell glob expansion
        let full_pattern = if Path::new(pattern).is_absolute() {
            pattern.to_string()
        } else {
            format!("{}/{pattern}", self.working_directory.display())
        };

        // Use shell globbing via ls
        let output = std::process::Command::new("sh")
            .args(["-c", &format!("ls -d {full_pattern} 2>/dev/null")])
            .output()
            .map_err(|e| format!("Failed to run glob: {e}"))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let results: Vec<String> = stdout.lines().map(String::from).filter(|l| !l.is_empty()).collect();
        Ok(results)
    }

    async fn initialize(&self) -> Result<(), String> {
        tokio::fs::create_dir_all(&self.working_directory)
            .await
            .map_err(|e| format!("Failed to create working directory: {e}"))
    }

    async fn cleanup(&self) -> Result<(), String> {
        Ok(())
    }

    fn working_directory(&self) -> &str {
        self.working_directory.to_str().unwrap_or(".")
    }

    fn platform(&self) -> &str {
        if cfg!(target_os = "macos") {
            "darwin"
        } else if cfg!(target_os = "linux") {
            "linux"
        } else if cfg!(target_os = "windows") {
            "windows"
        } else {
            "unknown"
        }
    }

    fn os_version(&self) -> String {
        #[cfg(unix)]
        {
            let output = std::process::Command::new("uname")
                .arg("-r")
                .output();
            match output {
                Ok(out) => {
                    let version = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    format!("{} {version}", self.platform())
                }
                Err(_) => self.platform().to_string(),
            }
        }
        #[cfg(not(unix))]
        {
            self.platform().to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("local_env_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn read_file_with_line_numbers() {
        let dir = temp_dir();
        std::fs::write(dir.join("test.txt"), "hello\nworld\nfoo").unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env.read_file("test.txt").await.unwrap();

        assert_eq!(result, "1 | hello\n2 | world\n3 | foo\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn read_file_line_number_padding() {
        let dir = temp_dir();
        let content: String = (1..=12).map(|i| format!("line {i}\n")).collect();
        std::fs::write(dir.join("padded.txt"), content.trim_end()).unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env.read_file("padded.txt").await.unwrap();

        assert!(result.starts_with(" 1 | line 1\n"));
        assert!(result.contains("12 | line 12\n"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn read_file_not_found() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env.read_file("nonexistent.txt").await;
        assert!(result.is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn write_file_creates_parent_dirs() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        env.write_file("sub/dir/test.txt", "content").await.unwrap();

        let written = std::fs::read_to_string(dir.join("sub/dir/test.txt")).unwrap();
        assert_eq!(written, "content");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn file_exists_true() {
        let dir = temp_dir();
        std::fs::write(dir.join("exists.txt"), "data").unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        assert!(env.file_exists("exists.txt").await.unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn file_exists_false() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        assert!(!env.file_exists("nope.txt").await.unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn list_directory_sorted() {
        let dir = temp_dir();
        std::fs::write(dir.join("b.txt"), "b").unwrap();
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        std::fs::create_dir(dir.join("c_dir")).unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let entries = env.list_directory(".").await.unwrap();

        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "a.txt");
        assert!(!entries[0].is_dir);
        assert!(entries[0].size.is_some());
        assert_eq!(entries[1].name, "b.txt");
        assert_eq!(entries[2].name, "c_dir");
        assert!(entries[2].is_dir);
        assert!(entries[2].size.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn exec_command_echo() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env
            .exec_command("echo", &["hello".into()], 5000)
            .await
            .unwrap();

        assert_eq!(result.stdout.trim(), "hello");
        assert_eq!(result.exit_code, 0);
        assert!(!result.timed_out);
        assert!(result.duration_ms < 5000);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn exec_command_exit_code() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env
            .exec_command("sh", &["-c".into(), "exit 42".into()], 5000)
            .await
            .unwrap();

        assert_eq!(result.exit_code, 42);
        assert!(!result.timed_out);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn exec_command_timeout() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env
            .exec_command("sleep", &["10".into()], 200)
            .await
            .unwrap();

        assert!(result.timed_out);
        assert_eq!(result.exit_code, -1);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn exec_command_stderr() {
        let dir = temp_dir();
        let env = LocalExecutionEnvironment::new(dir.clone());
        let result = env
            .exec_command("sh", &["-c".into(), "echo err >&2".into()], 5000)
            .await
            .unwrap();

        assert_eq!(result.stderr.trim(), "err");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn env_var_filtering() {
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "OPENAI_API_KEY"
        ));
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "ANTHROPIC_API_KEY"
        ));
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "DB_PASSWORD"
        ));
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "AWS_SECRET"
        ));
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "AUTH_TOKEN"
        ));
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "MY_CREDENTIAL"
        ));
        // Case insensitive
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "my_api_key"
        ));
        assert!(LocalExecutionEnvironment::should_filter_env_var(
            "Some_Secret"
        ));
        // Should not filter
        assert!(!LocalExecutionEnvironment::should_filter_env_var("PATH"));
        assert!(!LocalExecutionEnvironment::should_filter_env_var("HOME"));
        assert!(!LocalExecutionEnvironment::should_filter_env_var("EDITOR"));
        assert!(!LocalExecutionEnvironment::should_filter_env_var(
            "SECRET_PATH"
        ));
    }

    #[test]
    fn platform_is_known() {
        let env = LocalExecutionEnvironment::new(PathBuf::from("/tmp"));
        let platform = env.platform();
        assert!(
            platform == "darwin" || platform == "linux" || platform == "windows",
            "Unknown platform: {platform}"
        );
    }

    #[test]
    fn os_version_contains_platform() {
        let env = LocalExecutionEnvironment::new(PathBuf::from("/tmp"));
        let version = env.os_version();
        assert!(
            version.contains(env.platform()),
            "OS version should contain platform: {version}"
        );
    }

    #[test]
    fn working_directory_accessor() {
        let env = LocalExecutionEnvironment::new(PathBuf::from("/tmp/test_dir"));
        assert_eq!(env.working_directory(), "/tmp/test_dir");
    }

    #[tokio::test]
    async fn initialize_creates_directory() {
        let dir = std::env::temp_dir().join(format!("init_test_{}", uuid::Uuid::new_v4()));
        let env = LocalExecutionEnvironment::new(dir.clone());
        env.initialize().await.unwrap();
        assert!(dir.exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn grep_finds_matches() {
        let dir = temp_dir();
        std::fs::write(dir.join("test.rs"), "fn main() {\n    println!(\"hello\");\n}\n").unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let results = env
            .grep("println", "test.rs", &GrepOptions::default())
            .await
            .unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].contains("println"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn grep_case_insensitive() {
        let dir = temp_dir();
        std::fs::write(dir.join("test.txt"), "Hello\nhello\nHELLO\n").unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let results = env
            .grep(
                "hello",
                "test.txt",
                &GrepOptions {
                    case_insensitive: true,
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 3);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn grep_max_results() {
        let dir = temp_dir();
        std::fs::write(dir.join("test.txt"), "match1\nmatch2\nmatch3\nmatch4\n").unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let results = env
            .grep(
                "match",
                "test.txt",
                &GrepOptions {
                    max_results: Some(2),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        assert_eq!(results.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn glob_finds_files() {
        let dir = temp_dir();
        std::fs::write(dir.join("a.rs"), "").unwrap();
        std::fs::write(dir.join("b.rs"), "").unwrap();
        std::fs::write(dir.join("c.txt"), "").unwrap();

        let env = LocalExecutionEnvironment::new(dir.clone());
        let results = env.glob("*.rs").await.unwrap();

        assert_eq!(results.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn format_line_numbered_empty() {
        let result = LocalExecutionEnvironment::format_line_numbered("");
        assert_eq!(result, "");
    }

    #[test]
    fn format_line_numbered_single_line() {
        let result = LocalExecutionEnvironment::format_line_numbered("hello");
        assert_eq!(result, "1 | hello\n");
    }
}
