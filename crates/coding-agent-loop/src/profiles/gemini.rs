use crate::execution_env::ExecutionEnvironment;
use crate::provider_profile::ProviderProfile;
use crate::subagent::{
    make_close_agent_tool, make_send_input_tool, make_spawn_agent_tool, make_wait_tool,
    SessionFactory, SubAgentManager,
};
use crate::tool_registry::{RegisteredTool, ToolRegistry};
use crate::tools::{
    make_edit_file_tool, make_glob_tool, make_grep_tool, make_read_file_tool, make_shell_tool,
    make_write_file_tool,
};
use std::sync::Arc;
use unified_llm::types::ToolDefinition;

use super::{build_env_context_block_with, EnvContext};

fn make_list_dir_tool() -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "list_dir".into(),
            description: "List directory contents with depth control".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path to list"},
                    "depth": {"type": "integer", "description": "Depth of listing (default 1)"}
                },
                "required": ["path"]
            }),
        },
        executor: Arc::new(|args, env| {
            Box::pin(async move {
                let path = args["path"]
                    .as_str()
                    .ok_or_else(|| "path is required".to_string())?;

                let entries = env.list_directory(path).await?;
                let lines: Vec<String> = entries
                    .iter()
                    .map(|e| {
                        if e.is_dir {
                            format!("{}/", e.name)
                        } else {
                            e.name.clone()
                        }
                    })
                    .collect();
                Ok(lines.join("\n"))
            })
        }),
    }
}

pub struct GeminiProfile {
    model: String,
    registry: ToolRegistry,
}

impl GeminiProfile {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        let mut registry = ToolRegistry::new();

        registry.register(make_read_file_tool());
        registry.register(make_write_file_tool());
        registry.register(make_edit_file_tool());
        registry.register(make_shell_tool());
        registry.register(make_grep_tool());
        registry.register(make_glob_tool());
        registry.register(make_list_dir_tool());

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

impl ProviderProfile for GeminiProfile {
    fn id(&self) -> String {
        "gemini".into()
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
            "You are a coding assistant powered by Gemini. You help users with software engineering tasks \
including solving bugs, adding new functionality, refactoring code, and explaining code.\n\n\
{env_block}\n\n\
# Tools\n\
Use the provided tools to interact with the codebase and environment.\n\n\
- read_file: Read files to understand code before modifying. Use offset/limit for large files.\n\
- edit_file: Use search-and-replace editing. The old_string must exactly match existing text. Prefer editing existing files over creating new ones.\n\
- write_file: Use for creating new files or completely rewriting files.\n\
- shell: Execute shell commands. Default timeout is 10 seconds.\n\
- grep: Search file contents with regex patterns.\n\
- glob: Find files by name pattern. Results sorted by modification time.\n\
- list_dir: List directory contents with depth control.\n\n\
# Project Docs\n\
Look for GEMINI.md and AGENTS.md files in the project for project-specific instructions.\n\n\
# Coding Best Practices\n\
Write clean, maintainable code. Handle errors appropriately. Follow existing code conventions in the project.\
{docs_section}\
{user_section}"
        )
    }

    fn tools(&self) -> Vec<ToolDefinition> {
        self.registry.definitions()
    }

    fn provider_options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({
            "gemini": {
                "safety_settings": [
                    {
                        "category": "HARM_CATEGORY_DANGEROUS_CONTENT",
                        "threshold": "BLOCK_NONE"
                    }
                ]
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
        1_000_000
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_env::*;
    use async_trait::async_trait;
    use std::sync::Arc;

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
    fn gemini_profile_identity() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        assert_eq!(profile.id(), "gemini");
        assert_eq!(profile.model(), "gemini-2.0-flash");
    }

    #[test]
    fn gemini_profile_capabilities() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        assert!(profile.supports_reasoning());
        assert!(profile.supports_streaming());
        assert!(profile.supports_parallel_tool_calls());
        assert_eq!(profile.context_window_size(), 1_000_000);
    }

    #[test]
    fn gemini_system_prompt_contains_identity() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let env = TestEnv;
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &[], None);
        assert!(prompt.contains("You are a coding assistant powered by Gemini"));
        assert!(prompt.contains("solving bugs"));
        assert!(prompt.contains("adding new functionality"));
        assert!(prompt.contains("refactoring code"));
        assert!(prompt.contains("explaining code"));
    }

    #[test]
    fn gemini_system_prompt_contains_tool_guidance() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let env = TestEnv;
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &[], None);
        assert!(prompt.contains("read_file"));
        assert!(prompt.contains("edit_file"));
        assert!(prompt.contains("write_file"));
        assert!(prompt.contains("shell"));
        assert!(prompt.contains("grep"));
        assert!(prompt.contains("glob"));
        assert!(prompt.contains("list_dir"));
        assert!(prompt.contains("Default timeout is 10 seconds"));
    }

    #[test]
    fn gemini_system_prompt_contains_project_docs_convention() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let env = TestEnv;
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &[], None);
        assert!(prompt.contains("GEMINI.md"));
        assert!(prompt.contains("AGENTS.md"));
    }

    #[test]
    fn gemini_system_prompt_contains_coding_best_practices() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let env = TestEnv;
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &[], None);
        assert!(prompt.contains("clean, maintainable code"));
        assert!(prompt.contains("Handle errors appropriately"));
        assert!(prompt.contains("existing code conventions"));
    }

    #[test]
    fn gemini_system_prompt_contains_env_context() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let env = TestEnv;
        let prompt = profile.build_system_prompt(&env, &EnvContext::default(), &[], None);
        assert!(prompt.contains("# Environment"));
        assert!(prompt.contains("linux"));
    }

    #[test]
    fn gemini_provider_options_returns_safety_settings() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let options = profile.provider_options();
        assert!(options.is_some());
        let options = options.unwrap();
        let safety = &options["gemini"]["safety_settings"];
        assert!(safety.is_array());
        let settings = safety.as_array().unwrap();
        assert_eq!(settings.len(), 1);
        assert_eq!(settings[0]["category"], "HARM_CATEGORY_DANGEROUS_CONTENT");
        assert_eq!(settings[0]["threshold"], "BLOCK_NONE");
    }

    #[test]
    fn gemini_tools_registered() {
        let profile = GeminiProfile::new("gemini-2.0-flash");
        let names = profile.tool_registry().names();
        assert_eq!(names.len(), 7);
        assert!(names.contains(&"read_file".to_string()));
        assert!(names.contains(&"write_file".to_string()));
        assert!(names.contains(&"edit_file".to_string()));
        assert!(names.contains(&"shell".to_string()));
        assert!(names.contains(&"grep".to_string()));
        assert!(names.contains(&"glob".to_string()));
        assert!(names.contains(&"list_dir".to_string()));
    }

    #[test]
    fn gemini_subagent_tools_registered() {
        let mut profile = GeminiProfile::new("gemini-2.0-flash");
        let manager = Arc::new(tokio::sync::Mutex::new(
            crate::subagent::SubAgentManager::new(3),
        ));
        let factory: crate::subagent::SessionFactory = Arc::new(|| {
            panic!("should not be called");
        });
        profile.register_subagent_tools(manager, factory, 0);
        let names = profile.tool_registry().names();
        assert_eq!(names.len(), 11);
        assert!(names.contains(&"spawn_agent".to_string()));
        assert!(names.contains(&"send_input".to_string()));
        assert!(names.contains(&"wait".to_string()));
        assert!(names.contains(&"close_agent".to_string()));
    }
}
