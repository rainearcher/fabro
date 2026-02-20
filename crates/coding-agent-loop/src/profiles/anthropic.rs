use crate::config::SessionConfig;
use crate::execution_env::ExecutionEnvironment;
use crate::provider_profile::ProviderProfile;
use crate::subagent::{
    make_close_agent_tool, make_send_input_tool, make_spawn_agent_tool, make_wait_tool,
    SessionFactory, SubAgentManager,
};
use crate::tool_registry::ToolRegistry;
use crate::tools::{
    make_edit_file_tool, make_glob_tool, make_grep_tool, make_read_file_tool,
    make_shell_tool_with_config, make_write_file_tool,
};
use std::sync::Arc;
use unified_llm::types::ToolDefinition;

use super::{build_env_context_block_with, EnvContext};

pub struct AnthropicProfile {
    model: String,
    registry: ToolRegistry,
}

impl AnthropicProfile {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        let config = SessionConfig {
            default_command_timeout_ms: 120_000,
            ..SessionConfig::default()
        };
        let mut registry = ToolRegistry::new();

        registry.register(make_read_file_tool());
        registry.register(make_write_file_tool());
        registry.register(make_edit_file_tool());
        registry.register(make_shell_tool_with_config(&config));
        registry.register(make_grep_tool());
        registry.register(make_glob_tool());

        Self {
            model: model.into(),
            registry,
        }
    }

    pub fn register_subagent_tools(
        &mut self,
        manager: Arc<tokio::sync::Mutex<SubAgentManager>>,
        session_factory: SessionFactory,
        current_depth: usize,
    ) {
        self.registry.register(make_spawn_agent_tool(
            manager.clone(),
            session_factory,
            current_depth,
        ));
        self.registry
            .register(make_send_input_tool(manager.clone()));
        self.registry.register(make_wait_tool(manager.clone()));
        self.registry.register(make_close_agent_tool(manager));
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

    fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
        &mut self.registry
    }

    fn build_system_prompt(
        &self,
        env: &dyn ExecutionEnvironment,
        env_context: &EnvContext,
        project_docs: &[String],
        user_instructions: Option<&str>,
    ) -> String {
        let env_block = build_env_context_block_with(env, env_context);
        let docs_section = if project_docs.is_empty() {
            String::new()
        } else {
            format!("\n\n{}", project_docs.join("\n\n"))
        };
        let user_section = match user_instructions {
            Some(instructions) => format!("\n\n# User Instructions\n{instructions}"),
            None => String::new(),
        };

        format!(
            "You are Claude, an AI coding assistant by Anthropic. \
             You help users with software engineering tasks including solving bugs, \
             adding new functionality, refactoring code, and explaining code.\n\n\
             {env_block}\n\n\
             # Tools\n\
             Use the provided tools to interact with the codebase and environment.\n\n\
             ## read_file\n\
             Read files before editing them. Use offset/limit for large files.\n\n\
             ## edit_file\n\
             The old_string must be an exact match of existing text and must be unique in the file. \
             If old_string matches multiple locations, provide more surrounding context to make it unique. \
             Prefer editing existing files over creating new ones.\n\n\
             ## write_file\n\
             Use write_file only when creating new files. Prefer edit_file for modifying existing files.\n\n\
             ## shell\n\
             Use for running commands, tests, and builds. Default timeout is 120 seconds.\n\n\
             ## grep\n\
             Search file contents with regex patterns. Supports output modes: content, files_with_matches, count.\n\n\
             ## glob\n\
             Find files by name pattern. Results sorted by modification time (newest first).\n\n\
             # Coding Best Practices\n\
             Write clean, maintainable code. Handle errors appropriately. \
             Follow existing code conventions in the project.\
             {docs_section}\
             {user_section}"
        )
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.registry.definitions()
    }

    fn provider_options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "anthropic": {
                "beta_headers": ["interleaved-thinking-2025-05-14"]
            }
        }))
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
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &[], None);
        assert!(prompt.contains("You are Claude, an AI coding assistant by Anthropic"));
        assert!(prompt.contains("# Environment"));
        assert!(prompt.contains("linux"));
        assert!(prompt.contains("/home/test"));
        assert!(prompt.contains("# Tools"));
        // Verify expanded tool guidance
        assert!(
            prompt.contains("old_string must be"),
            "prompt should contain edit_file guidance about old_string"
        );
        assert!(
            prompt.contains("exact match"),
            "prompt should contain edit_file guidance about exact match"
        );
        assert!(
            prompt.contains("Read files before editing"),
            "prompt should contain read_file guidance"
        );
        assert!(
            prompt.contains("Default timeout is 120 seconds"),
            "prompt should contain shell timeout guidance"
        );
        assert!(
            prompt.contains("Write clean, maintainable code"),
            "prompt should contain coding best practices"
        );
    }

    #[test]
    fn anthropic_system_prompt_includes_project_docs() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        let env = TestEnv;
        let docs = vec!["# Project README".into(), "# CONTRIBUTING guide".into()];
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &docs, None);
        assert!(prompt.contains("# Project README"));
        assert!(prompt.contains("# CONTRIBUTING guide"));
    }

    #[test]
    fn anthropic_system_prompt_includes_env_context() {
        let profile = AnthropicProfile::new("claude-opus-4-6");
        let env = TestEnv;
        let ctx = EnvContext {
            git_branch: Some("feature-branch".into()),
            is_git_repo: true,
            date: "2026-02-20".into(),
            model_name: "claude-opus-4-6".into(),
        };
        let prompt = profile.build_system_prompt(&env, &ctx, &[], None);
        assert!(prompt.contains("Git branch: feature-branch"));
        assert!(prompt.contains("Is a git repository: true"));
        assert!(prompt.contains("Date: 2026-02-20"));
        assert!(prompt.contains("Model: claude-opus-4-6"));
    }

    #[test]
    fn anthropic_system_prompt_includes_user_instructions() {
        let profile = AnthropicProfile::new("claude-opus-4-6");
        let env = TestEnv;
        let ctx = EnvContext::default();
        let prompt = profile.build_system_prompt(&env, &ctx, &[], Some("Always write tests first"));
        assert!(prompt.contains("Always write tests first"));
        assert!(prompt.contains("# User Instructions"));
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

    #[test]
    fn anthropic_provider_options_include_beta_headers() {
        let profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        let options = profile.provider_options();
        assert!(options.is_some(), "provider_options should return Some");
        let options = options.unwrap();
        let beta_headers = &options["anthropic"]["beta_headers"];
        assert!(beta_headers.is_array(), "beta_headers should be an array");
        let headers: Vec<&str> = beta_headers
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert!(
            headers.contains(&"interleaved-thinking-2025-05-14"),
            "beta_headers should contain interleaved-thinking header"
        );
    }

    #[test]
    fn anthropic_register_subagent_tools() {
        use crate::subagent::{SessionFactory, SubAgentManager};
        use std::sync::Arc;

        let mut profile = AnthropicProfile::new("claude-sonnet-4-20250514");
        assert_eq!(profile.tool_registry().names().len(), 6);

        let manager = Arc::new(tokio::sync::Mutex::new(SubAgentManager::new(3)));
        let factory: SessionFactory = Arc::new(|| {
            panic!("should not be called in test");
        });

        profile.register_subagent_tools(manager, factory, 0);

        let names = profile.tool_registry().names();
        assert_eq!(names.len(), 10, "should have 6 base + 4 subagent tools");
        assert!(names.contains(&"spawn_agent".to_string()));
        assert!(names.contains(&"send_input".to_string()));
        assert!(names.contains(&"wait".to_string()));
        assert!(names.contains(&"close_agent".to_string()));
    }
}
