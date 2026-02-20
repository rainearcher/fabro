use crate::execution_env::ExecutionEnvironment;
use crate::provider_profile::ProviderProfile;
use crate::tool_registry::ToolRegistry;
use unified_llm::types::ToolDefinition;

use super::{build_env_context_block, stub_tool};

pub struct AnthropicProfile {
    model: String,
    registry: ToolRegistry,
}

impl AnthropicProfile {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        let mut registry = ToolRegistry::new();

        registry.register(stub_tool(
            "read_file",
            "Read the contents of a file at the given path",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to read" }
                },
                "required": ["path"]
            }),
        ));

        registry.register(stub_tool(
            "write_file",
            "Write content to a file at the given path",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to write" },
                    "content": { "type": "string", "description": "Content to write" }
                },
                "required": ["path", "content"]
            }),
        ));

        registry.register(stub_tool(
            "edit_file",
            "Edit a file by replacing old text with new text",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Path to the file to edit" },
                    "old_text": { "type": "string", "description": "Text to find and replace" },
                    "new_text": { "type": "string", "description": "Replacement text" }
                },
                "required": ["path", "old_text", "new_text"]
            }),
        ));

        registry.register(stub_tool(
            "shell",
            "Execute a shell command",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "command": { "type": "string", "description": "Shell command to execute" },
                    "timeout_ms": { "type": "integer", "description": "Timeout in milliseconds" }
                },
                "required": ["command"]
            }),
        ));

        registry.register(stub_tool(
            "grep",
            "Search for a pattern in files",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Regex pattern to search for" },
                    "path": { "type": "string", "description": "Directory or file to search in" }
                },
                "required": ["pattern", "path"]
            }),
        ));

        registry.register(stub_tool(
            "glob",
            "Find files matching a glob pattern",
            serde_json::json!({
                "type": "object",
                "properties": {
                    "pattern": { "type": "string", "description": "Glob pattern to match files" }
                },
                "required": ["pattern"]
            }),
        ));

        Self {
            model: model.into(),
            registry,
        }
    }
}

impl ProviderProfile for AnthropicProfile {
    fn id(&self) -> String {
        "anthropic".into()
    }

    fn model(&self) -> String {
        self.model.clone()
    }

    fn tool_registry(&self) -> &ToolRegistry {
        &self.registry
    }

    fn build_system_prompt(
        &self,
        env: &dyn ExecutionEnvironment,
        project_docs: &[String],
    ) -> String {
        let env_block = build_env_context_block(env);
        let docs_section = if project_docs.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", project_docs.join("\n\n"))
        };

        format!(
            "You are Claude, an AI assistant by Anthropic. You help users with software engineering tasks.\n\n\
             {env_block}\n\n\
             # Tools\n\
             Use the provided tools to interact with the codebase and environment.\
             {docs_section}"
        )
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.registry.definitions()
    }

    fn provider_options(&self) -> Option<serde_json::Value> {
        None
    }

    fn supports_reasoning(&self) -> bool {
        true
    }

    fn supports_streaming(&self) -> bool {
        true
    }

    fn supports_parallel_tool_calls(&self) -> bool {
        true
    }

    fn context_window_size(&self) -> usize {
        200_000
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
    fn anthropic_profile_identity() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        assert_eq!(profile.id(), "anthropic");
        assert_eq!(profile.model(), "claude-sonnet-4-20250514");
    }

    #[test]
    fn anthropic_profile_capabilities() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        assert!(profile.supports_reasoning());
        assert!(profile.supports_streaming());
        assert!(profile.supports_parallel_tool_calls());
        assert_eq!(profile.context_window_size(), 200_000);
    }

    #[test]
    fn anthropic_system_prompt_contains_env_context() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        let env = TestEnv;
        let prompt = profile.build_system_prompt(&env, &[]);
        assert!(prompt.contains("You are Claude, an AI assistant by Anthropic"));
        assert!(prompt.contains("# Environment"));
        assert!(prompt.contains("linux"));
        assert!(prompt.contains("/home/test"));
        assert!(prompt.contains("# Tools"));
    }

    #[test]
    fn anthropic_system_prompt_includes_project_docs() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        let env = TestEnv;
        let docs = vec!["# Project README".into(), "# CONTRIBUTING guide".into()];
        let prompt = profile.build_system_prompt(&env, &docs);
        assert!(prompt.contains("# Project README"));
        assert!(prompt.contains("# CONTRIBUTING guide"));
    }

    #[test]
    fn anthropic_tools_registered() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        let names = profile.tool_registry().names();
        assert_eq!(names.len(), 6);
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"write_file".to_string()));
        assert!(names.contains(&"edit_file".to_string()));
        assert!(names.contains(&"shell".to_string()));
        assert!(names.contains(&"grep".to_string()));
        assert!(names.contains(&"glob".to_string()));
    }
}
