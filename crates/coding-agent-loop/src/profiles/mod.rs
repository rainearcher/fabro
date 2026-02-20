pub mod anthropic;
pub mod gemini;
pub mod openai;

pub use anthropic::AnthropicProfile;
pub use gemini::GeminiProfile;
pub use openai::OpenAiProfile;

use crate::execution_env::ExecutionEnvironment;
use crate::tool_registry::RegisteredTool;
use std::sync::Arc;
use unified_llm::types::ToolDefinition;

#[must_use]
pub fn build_env_context_block(env: &dyn ExecutionEnvironment) -> String {
    format!(
        "# Environment\n- Working directory: {}\n- Platform: {}\n- OS: {}",
        env.working_directory(),
        env.platform(),
        env.os_version()
    )
}

#[must_use]
pub fn stub_tool(name: &str, description: &str, parameters: serde_json::Value) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: name.into(),
            description: description.into(),
            parameters,
        },
        executor: Arc::new(|_args, _env| {
            Box::pin(async { Err("Tool not yet connected to execution environment".into()) })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_env::*;
    use async_trait::async_trait;

    struct TestEnv;

    #[async_trait]
    impl ExecutionEnvironment for TestEnv {
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
            _: u64,
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
        assert!(block.contains("# Environment"));
        assert!(block.contains("linux"));
        assert!(block.contains("/home/test"));
        assert!(block.contains("Linux 6.1.0"));
    }
}
