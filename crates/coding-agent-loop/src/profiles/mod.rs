pub mod anthropic;
pub mod gemini;
pub mod openai;

pub use anthropic::AnthropicProfile;
pub use gemini::GeminiProfile;
pub use openai::OpenAiProfile;

use crate::execution_env::ExecutionEnvironment;

/// Additional context for building environment blocks
#[derive(Default)]
pub struct EnvContext {
    pub git_branch: Option<String>,
    pub is_git_repo: bool,
    pub date: String,
    pub model_name: String,
    pub knowledge_cutoff: String,
    pub git_status_short: Option<String>,
    pub git_recent_commits: Option<String>,
}

#[must_use]
pub fn build_env_context_block(env: &dyn ExecutionEnvironment) -> String {
    build_env_context_block_with(env, &EnvContext::default())
}

#[must_use]
pub fn build_env_context_block_with(env: &dyn ExecutionEnvironment, ctx: &EnvContext) -> String {
    let mut lines = vec![
        "<environment>".to_string(),
        format!("Working directory: {}", env.working_directory()),
        format!("Is git repository: {}", ctx.is_git_repo),
    ];

    if let Some(ref branch) = ctx.git_branch {
        lines.push(format!("Git branch: {branch}"));
    }

    lines.push(format!("Platform: {}", env.platform()));
    lines.push(format!("OS version: {}", env.os_version()));

    if !ctx.date.is_empty() {
        lines.push(format!("Today's date: {}", ctx.date));
    }
    if !ctx.model_name.is_empty() {
        lines.push(format!("Model: {}", ctx.model_name));
    }
    if !ctx.knowledge_cutoff.is_empty() {
        lines.push(format!("Knowledge cutoff: {}", ctx.knowledge_cutoff));
    }

    lines.push("</environment>".to_string());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_env::*;
    use async_trait::async_trait;

    struct TestEnv;

    #[async_trait]
    impl ExecutionEnvironment for TestEnv {
        async fn read_file(&self, _: &str, _: Option<usize>, _: Option<usize>) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _: &str, _: Option<usize>) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(
            &self,
            _: &str,
            _: u64,
            _: Option<&str>,
            _: Option<&std::collections::HashMap<String, String>>,
        ) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 0,
                timed_out: false,
                duration_ms: 0,
            })
        }
        async fn grep(
            &self,
            _: &str,
            _: &str,
            _: &GrepOptions,
        ) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn glob(&self, _: &str, _: Option<&str>) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn initialize(&self) -> Result<(), String> {
            Ok(())
        }
        async fn cleanup(&self) -> Result<(), String> {
            Ok(())
        }
        fn working_directory(&self) -> &str {
            "/home/test"
        }
        fn platform(&self) -> &str {
            "linux"
        }
        fn os_version(&self) -> String {
            "Linux 6.1.0".into()
        }
    }

    #[test]
    fn env_context_block_contains_platform() {
        let env = TestEnv;
        let block = build_env_context_block(&env);
        assert!(block.contains("<environment>"));
        assert!(block.contains("</environment>"));
        assert!(block.contains("linux"));
        assert!(block.contains("/home/test"));
        assert!(block.contains("Linux 6.1.0"));
    }

    #[test]
    fn env_context_block_with_extra_context() {
        let env = TestEnv;
        let ctx = EnvContext {
            git_branch: Some("main".into()),
            is_git_repo: true,
            date: "2026-02-20".into(),
            model_name: "claude-opus-4-6".into(),
            knowledge_cutoff: "May 2025".into(),
            git_status_short: None,
            git_recent_commits: None,
        };
        let block = build_env_context_block_with(&env, &ctx);
        assert!(block.contains("Git branch: main"));
        assert!(block.contains("Is git repository: true"));
        assert!(block.contains("Today's date: 2026-02-20"));
        assert!(block.contains("Model: claude-opus-4-6"));
        assert!(block.contains("Knowledge cutoff: May 2025"));
    }
}
