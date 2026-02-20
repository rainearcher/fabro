use crate::execution_env::GrepOptions;
use crate::tool_registry::RegisteredTool;
use std::fmt::Write;
use std::sync::Arc;
use unified_llm::types::ToolDefinition;

#[must_use]
pub fn make_read_file_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "read_file".into(),
            description: "Read the contents of a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Absolute path to the file"},
                    "offset": {"type": "integer", "description": "1-based line number to start reading from"},
                    "limit": {"type": "integer", "description": "Number of lines to read"}
                },
                "required": ["file_path"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let file_path = args["file_path"]
                    .as_str()
                    .ok_or_else(|| "file_path is required".to_string())?;
                let offset = args.get("offset").and_then(serde_json::Value::as_u64);
                let limit = args.get("limit").and_then(serde_json::Value::as_u64);

                let content = env.read_file(file_path).await?;

                if offset.is_none() && limit.is_none() {
                    return Ok(content);
                }

                #[allow(clippy::cast_possible_truncation)]
                let offset = offset.unwrap_or(1) as usize;
                let lines: Vec<&str> = content.lines().collect();
                let start = if offset > 0 { offset - 1 } else { 0 };
                #[allow(clippy::cast_possible_truncation)]
                let selected: Vec<&str> = match limit {
                    Some(lim) => lines.into_iter().skip(start).take(lim as usize).collect(),
                    None => lines.into_iter().skip(start).collect(),
                };
                Ok(selected.join("\n"))
            })
        }),
    }
}

#[must_use]
pub fn make_write_file_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "write_file".into(),
            description: "Write content to a file".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Absolute path to the file"},
                    "content": {"type": "string", "description": "Content to write to the file"}
                },
                "required": ["file_path", "content"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let file_path = args["file_path"]
                    .as_str()
                    .ok_or_else(|| "file_path is required".to_string())?;
                let content = args["content"]
                    .as_str()
                    .ok_or_else(|| "content is required".to_string())?;

                env.write_file(file_path, content).await?;
                Ok(format!("Successfully wrote to {file_path}"))
            })
        }),
    }
}

#[must_use]
pub fn make_edit_file_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "edit_file".into(),
            description: "Edit a file by replacing a string".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "file_path": {"type": "string", "description": "Absolute path to the file"},
                    "old_string": {"type": "string", "description": "The string to find and replace"},
                    "new_string": {"type": "string", "description": "The replacement string"},
                    "replace_all": {"type": "boolean", "description": "Replace all occurrences (default false)"}
                },
                "required": ["file_path", "old_string", "new_string"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let file_path = args["file_path"]
                    .as_str()
                    .ok_or_else(|| "file_path is required".to_string())?;
                let old_string = args["old_string"]
                    .as_str()
                    .ok_or_else(|| "old_string is required".to_string())?;
                let new_string = args["new_string"]
                    .as_str()
                    .ok_or_else(|| "new_string is required".to_string())?;
                let replace_all = args
                    .get("replace_all")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false);

                let numbered_content = env.read_file(file_path).await?;

                // Strip line numbers: each line looks like "  1 | content" or " 10 | content"
                let raw_lines: Vec<&str> = numbered_content
                    .lines()
                    .map(|line| {
                        line.find(" | ")
                            .map_or(line, |idx| &line[idx + 3..])
                    })
                    .collect();
                let raw_content = raw_lines.join("\n");

                let count = raw_content.matches(old_string).count();
                if count == 0 {
                    return Err("old_string not found in file".to_string());
                }
                if count > 1 && !replace_all {
                    return Err(format!(
                        "old_string is not unique in file (found {count} occurrences). Use replace_all or provide more context"
                    ));
                }

                let new_content = if replace_all {
                    raw_content.replace(old_string, new_string)
                } else {
                    raw_content.replacen(old_string, new_string, 1)
                };

                env.write_file(file_path, &new_content).await?;
                Ok(format!("Successfully edited {file_path}"))
            })
        }),
    }
}

#[must_use]
pub fn make_shell_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "shell".into(),
            description: "Execute a shell command".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The shell command to execute"},
                    "timeout_ms": {"type": "integer", "description": "Timeout in milliseconds (default 10000)"}
                },
                "required": ["command"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let command = args["command"]
                    .as_str()
                    .ok_or_else(|| "command is required".to_string())?;
                let timeout_ms = args
                    .get("timeout_ms")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(10000);

                let result = env
                    .exec_command(
                        "/bin/bash",
                        &["-c".into(), command.into()],
                        timeout_ms,
                    )
                    .await?;

                let mut output = String::new();
                if result.timed_out {
                    output.push_str("Command timed out.\n");
                }
                let _ = write!(
                    output,
                    "Exit code: {}\nstdout:\n{}\nstderr:\n{}",
                    result.exit_code, result.stdout, result.stderr
                );
                Ok(output)
            })
        }),
    }
}

#[must_use]
pub fn make_grep_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "grep".into(),
            description: "Search file contents with a regex pattern".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern to search for"},
                    "path": {"type": "string", "description": "Path to search in (default \".\")"},
                    "glob_filter": {"type": "string", "description": "Glob pattern to filter files"},
                    "case_insensitive": {"type": "boolean", "description": "Case insensitive search"},
                    "max_results": {"type": "integer", "description": "Maximum number of results"}
                },
                "required": ["pattern"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let pattern = args["pattern"]
                    .as_str()
                    .ok_or_else(|| "pattern is required".to_string())?;
                let path = args
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or(".");

                #[allow(clippy::cast_possible_truncation)]
                let options = GrepOptions {
                    glob_filter: args
                        .get("glob_filter")
                        .and_then(serde_json::Value::as_str)
                        .map(String::from),
                    case_insensitive: args
                        .get("case_insensitive")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    max_results: args
                        .get("max_results")
                        .and_then(serde_json::Value::as_u64)
                        .map(|v| v as usize),
                };

                let results = env.grep(pattern, path, &options).await?;
                Ok(results.join("\n"))
            })
        }),
    }
}

#[must_use]
pub fn make_glob_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "glob".into(),
            description: "Find files matching a glob pattern".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern to match files"}
                },
                "required": ["pattern"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let pattern = args["pattern"]
                    .as_str()
                    .ok_or_else(|| "pattern is required".to_string())?;

                let results = env.glob(pattern).await?;
                Ok(results.join("\n"))
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_env::*;
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct ReadFileEnv {
        content: String,
    }

    #[async_trait]
    impl ExecutionEnvironment for ReadFileEnv {
        async fn read_file(&self, _path: &str) -> Result<String, String> {
            Ok(self.content.clone())
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
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
            String::new()
        }
    }

    struct WriteFileEnv {
        written: Mutex<Option<(String, String)>>,
    }

    #[async_trait]
    impl ExecutionEnvironment for WriteFileEnv {
        async fn read_file(&self, _path: &str) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, path: &str, content: &str) -> Result<(), String> {
            *self.written.lock().unwrap() = Some((path.into(), content.into()));
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
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
            String::new()
        }
    }

    struct EditFileEnv {
        content: String,
        written: Mutex<Option<String>>,
    }

    #[async_trait]
    impl ExecutionEnvironment for EditFileEnv {
        async fn read_file(&self, _path: &str) -> Result<String, String> {
            Ok(self.content.clone())
        }
        async fn write_file(&self, _path: &str, content: &str) -> Result<(), String> {
            *self.written.lock().unwrap() = Some(content.into());
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
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
            String::new()
        }
    }

    struct ShellEnv {
        result: ExecResult,
    }

    #[async_trait]
    impl ExecutionEnvironment for ShellEnv {
        async fn read_file(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(self.result.clone())
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
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
            String::new()
        }
    }

    struct ShellCapturingEnv {
        captured_timeout: Mutex<Option<u64>>,
    }

    #[async_trait]
    impl ExecutionEnvironment for ShellCapturingEnv {
        async fn read_file(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(
            &self,
            _: &str,
            _: &[String],
            timeout_ms: u64,
        ) -> Result<ExecResult, String> {
            *self.captured_timeout.lock().unwrap() = Some(timeout_ms);
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
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
            String::new()
        }
    }

    struct GrepEnv {
        results: Vec<String>,
    }

    #[async_trait]
    impl ExecutionEnvironment for GrepEnv {
        async fn read_file(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(self.results.clone())
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(vec![])
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
            String::new()
        }
    }

    struct GlobEnv {
        results: Vec<String>,
    }

    #[async_trait]
    impl ExecutionEnvironment for GlobEnv {
        async fn read_file(&self, _: &str) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(&self, _: &str, _: &[String], _: u64) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(&self, _: &str, _: &str, _: &GrepOptions) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str) -> Result<Vec<String>, String> {
            Ok(self.results.clone())
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
            String::new()
        }
    }

    #[tokio::test]
    async fn read_file_returns_content() {
        let tool = make_read_file_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(ReadFileEnv {
            content: "  1 | hello\n  2 | world".into(),
        });
        let result = (tool.executor)(serde_json::json!({"file_path": "/test.txt"}), env).await;
        assert_eq!(result.unwrap(), "  1 | hello\n  2 | world");
    }

    #[tokio::test]
    async fn read_file_with_offset_and_limit() {
        let tool = make_read_file_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(ReadFileEnv {
            content: "  1 | line1\n  2 | line2\n  3 | line3\n  4 | line4".into(),
        });
        let result = (tool.executor)(
            serde_json::json!({"file_path": "/test.txt", "offset": 2, "limit": 2}),
            env,
        )
        .await;
        assert_eq!(result.unwrap(), "  2 | line2\n  3 | line3");
    }

    #[tokio::test]
    async fn write_file_calls_env() {
        let tool = make_write_file_tool();
        let env = Arc::new(WriteFileEnv {
            written: Mutex::new(None),
        });
        let env_clone: Arc<dyn ExecutionEnvironment> = env.clone();
        let result = (tool.executor)(
            serde_json::json!({"file_path": "/out.txt", "content": "hello"}),
            env_clone,
        )
        .await;
        assert_eq!(result.unwrap(), "Successfully wrote to /out.txt");
        let written = env.written.lock().unwrap();
        let (path, content) = written.as_ref().unwrap();
        assert_eq!(path, "/out.txt");
        assert_eq!(content, "hello");
    }

    #[tokio::test]
    async fn edit_file_replaces_match() {
        let tool = make_edit_file_tool();
        let env = Arc::new(EditFileEnv {
            content: "  1 | hello world".into(),
            written: Mutex::new(None),
        });
        let env_clone: Arc<dyn ExecutionEnvironment> = env.clone();
        let result = (tool.executor)(
            serde_json::json!({
                "file_path": "/f.txt",
                "old_string": "hello",
                "new_string": "goodbye"
            }),
            env_clone,
        )
        .await;
        assert_eq!(result.unwrap(), "Successfully edited /f.txt");
        let written = env.written.lock().unwrap();
        assert_eq!(written.as_ref().unwrap(), "goodbye world");
    }

    #[tokio::test]
    async fn edit_file_not_found_error() {
        let tool = make_edit_file_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(EditFileEnv {
            content: "  1 | hello world".into(),
            written: Mutex::new(None),
        });
        let result = (tool.executor)(
            serde_json::json!({
                "file_path": "/f.txt",
                "old_string": "missing",
                "new_string": "replacement"
            }),
            env,
        )
        .await;
        assert_eq!(result.unwrap_err(), "old_string not found in file");
    }

    #[tokio::test]
    async fn edit_file_not_unique_error() {
        let tool = make_edit_file_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(EditFileEnv {
            content: "  1 | aa bb aa".into(),
            written: Mutex::new(None),
        });
        let result = (tool.executor)(
            serde_json::json!({
                "file_path": "/f.txt",
                "old_string": "aa",
                "new_string": "cc"
            }),
            env,
        )
        .await;
        let err = result.unwrap_err();
        assert!(err.contains("not unique"));
        assert!(err.contains("2 occurrences"));
    }

    #[tokio::test]
    async fn edit_file_replace_all() {
        let tool = make_edit_file_tool();
        let env = Arc::new(EditFileEnv {
            content: "  1 | aa bb aa".into(),
            written: Mutex::new(None),
        });
        let env_clone: Arc<dyn ExecutionEnvironment> = env.clone();
        let result = (tool.executor)(
            serde_json::json!({
                "file_path": "/f.txt",
                "old_string": "aa",
                "new_string": "cc",
                "replace_all": true
            }),
            env_clone,
        )
        .await;
        assert_eq!(result.unwrap(), "Successfully edited /f.txt");
        let written = env.written.lock().unwrap();
        assert_eq!(written.as_ref().unwrap(), "cc bb cc");
    }

    #[tokio::test]
    async fn shell_basic_command() {
        let tool = make_shell_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(ShellEnv {
            result: ExecResult {
                stdout: "hello".into(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 10,
            },
        });
        let result = (tool.executor)(serde_json::json!({"command": "echo hello"}), env).await;
        let output = result.unwrap();
        assert!(output.contains("Exit code: 0"));
        assert!(output.contains("hello"));
    }

    #[tokio::test]
    async fn shell_with_timeout() {
        let tool = make_shell_tool();
        let env = Arc::new(ShellCapturingEnv {
            captured_timeout: Mutex::new(None),
        });
        let env_clone: Arc<dyn ExecutionEnvironment> = env.clone();
        let _result = (tool.executor)(
            serde_json::json!({"command": "sleep 1", "timeout_ms": 5000}),
            env_clone,
        )
        .await;
        assert_eq!(*env.captured_timeout.lock().unwrap(), Some(5000));
    }

    #[tokio::test]
    async fn shell_nonzero_exit_code() {
        let tool = make_shell_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(ShellEnv {
            result: ExecResult {
                stdout: String::new(),
                stderr: "error".into(),
                exit_code: 1,
                timed_out: false,
                duration_ms: 10,
            },
        });
        let result = (tool.executor)(serde_json::json!({"command": "false"}), env).await;
        let output = result.unwrap();
        assert!(output.contains("Exit code: 1"));
        assert!(output.contains("error"));
    }

    #[tokio::test]
    async fn shell_timeout_output() {
        let tool = make_shell_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(ShellEnv {
            result: ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: -1,
                timed_out: true,
                duration_ms: 10000,
            },
        });
        let result = (tool.executor)(serde_json::json!({"command": "sleep 100"}), env).await;
        let output = result.unwrap();
        assert!(output.starts_with("Command timed out.\n"));
    }

    #[tokio::test]
    async fn grep_basic() {
        let tool = make_grep_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(GrepEnv {
            results: vec!["src/main.rs:10:fn main()".into(), "src/lib.rs:5:pub fn".into()],
        });
        let result = (tool.executor)(serde_json::json!({"pattern": "fn"}), env).await;
        let output = result.unwrap();
        assert!(output.contains("src/main.rs:10:fn main()"));
        assert!(output.contains("src/lib.rs:5:pub fn"));
    }

    #[tokio::test]
    async fn glob_basic() {
        let tool = make_glob_tool();
        let env: Arc<dyn ExecutionEnvironment> = Arc::new(GlobEnv {
            results: vec!["src/main.rs".into(), "src/lib.rs".into()],
        });
        let result = (tool.executor)(serde_json::json!({"pattern": "src/**/*.rs"}), env).await;
        let output = result.unwrap();
        assert!(output.contains("src/main.rs"));
        assert!(output.contains("src/lib.rs"));
    }
}
