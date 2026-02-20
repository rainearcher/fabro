use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct ExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub timed_out: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub struct GrepOptions {
    pub glob_filter: Option<String>,
    pub case_insensitive: bool,
    pub max_results: Option<usize>,
}

#[async_trait]
pub trait ExecutionEnvironment: Send + Sync {
    async fn read_file(&self, path: &str) -> Result<String, String>;
    async fn write_file(&self, path: &str, content: &str) -> Result<(), String>;
    async fn file_exists(&self, path: &str) -> Result<bool, String>;
    async fn list_directory(&self, path: &str) -> Result<Vec<DirEntry>, String>;
    async fn exec_command(
        &self,
        command: &str,
        args: &[String],
        timeout_ms: u64,
    ) -> Result<ExecResult, String>;
    async fn grep(
        &self,
        pattern: &str,
        path: &str,
        options: &GrepOptions,
    ) -> Result<Vec<String>, String>;
    async fn glob(&self, pattern: &str) -> Result<Vec<String>, String>;
    async fn initialize(&self) -> Result<(), String>;
    async fn cleanup(&self) -> Result<(), String>;
    fn working_directory(&self) -> &str;
    fn platform(&self) -> &str;
    fn os_version(&self) -> String;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct MockEnv;

    #[async_trait]
    impl ExecutionEnvironment for MockEnv {
        async fn read_file(&self, _path: &str) -> Result<String, String> {
            Ok("hello".into())
        }
        async fn write_file(&self, _path: &str, _content: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _path: &str) -> Result<bool, String> {
            Ok(true)
        }
        async fn list_directory(&self, _path: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![DirEntry {
                name: "test.rs".into(),
                is_dir: false,
                size: Some(100),
            }])
        }
        async fn exec_command(
            &self,
            _command: &str,
            _args: &[String],
            _timeout_ms: u64,
        ) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: "output".into(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 10,
            })
        }
        async fn grep(
            &self,
            _pattern: &str,
            _path: &str,
            _options: &GrepOptions,
        ) -> Result<Vec<String>, String> {
            Ok(vec!["match".into()])
        }
        async fn glob(&self, _pattern: &str) -> Result<Vec<String>, String> {
            Ok(vec!["file.rs".into()])
        }
        async fn initialize(&self) -> Result<(), String> {
            Ok(())
        }
        async fn cleanup(&self) -> Result<(), String> {
            Ok(())
        }
        fn working_directory(&self) -> &str {
            "/tmp"
        }
        fn platform(&self) -> &str {
            "darwin"
        }
        fn os_version(&self) -> String {
            "Darwin 24.0.0".into()
        }
    }

    #[tokio::test]
    async fn mock_env_read_file() {
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(MockEnv);
        let result = env.read_file("test.rs").await.unwrap();
        assert_eq!(result, "hello");
    }

    #[tokio::test]
    async fn mock_env_exec_command() {
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(MockEnv);
        let result = env.exec_command("echo", &[], 5000).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert!(!result.timed_out);
    }

    #[tokio::test]
    async fn mock_env_list_directory() {
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(MockEnv);
        let entries = env.list_directory("/tmp").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "test.rs");
        assert!(!entries[0].is_dir);
    }

    #[test]
    fn exec_result_fields() {
        let result = ExecResult {
            stdout: "out".into(),
            stderr: "err".into(),
            exit_code: 1,
            timed_out: true,
            duration_ms: 5000,
        };
        assert_eq!(result.exit_code, 1);
        assert!(result.timed_out);
        assert_eq!(result.duration_ms, 5000);
    }

    #[test]
    fn dir_entry_fields() {
        let entry = DirEntry {
            name: "src".into(),
            is_dir: true,
            size: None,
        };
        assert_eq!(entry.name, "src");
        assert!(entry.is_dir);
        assert!(entry.size.is_none());
    }

    #[test]
    fn grep_options_defaults() {
        let opts = GrepOptions::default();
        assert!(opts.glob_filter.is_none());
        assert!(!opts.case_insensitive);
        assert!(opts.max_results.is_none());
    }

    #[test]
    fn mock_env_platform() {
        let env = MockEnv;
        assert_eq!(env.platform(), "darwin");
        assert_eq!(env.working_directory(), "/tmp");
        assert_eq!(env.os_version(), "Darwin 24.0.0");
    }
}
