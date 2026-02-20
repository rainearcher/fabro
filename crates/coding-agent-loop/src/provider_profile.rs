use crate::execution_env::ExecutionEnvironment;
use crate::tool_registry::ToolRegistry;
use unified_llm::types::ToolDefinition;

pub trait ProviderProfile: Send + Sync {
    fn id(&self) -> String;
    fn model(&self) -> String;
    fn tool_registry(&self) -> &ToolRegistry;
    fn build_system_prompt(
        &self,
        env: &dyn ExecutionEnvironment,
        project_docs: &[String],
    ) -> String;
    fn tools(&self) -> Vec<ToolDefinition>;
    fn provider_options(&self) -> Option<serde_json::Value>;
    fn supports_reasoning(&self) -> bool;
    fn supports_streaming(&self) -> bool;
    fn supports_parallel_tool_calls(&self) -> bool;
    fn context_window_size(&self) -> usize;
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

    struct TestProfile {
        registry: ToolRegistry,
    }

    impl TestProfile {
        fn new() -> Self {
            Self {
                registry: ToolRegistry::new(),
            }
        }
    }

    impl ProviderProfile for TestProfile {
        fn id(&self) -> String {
            "test-provider".into()
        }
        fn model(&self) -> String {
            "test-model".into()
        }
        fn tool_registry(&self) -> &ToolRegistry {
            &self.registry
        }
        fn build_system_prompt(
            &self,
            env: &dyn ExecutionEnvironment,
            project_docs: &[String],
        ) -> String {
            format!(
                "You are working on {}. Docs: {}",
                env.platform(),
                project_docs.len()
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
            false
        }
        fn context_window_size(&self) -> usize {
            200_000
        }
    }

    #[test]
    fn profile_id_and_model() {
        let profile = TestProfile::new();
        assert_eq!(profile.id(), "test-provider");
        assert_eq!(profile.model(), "test-model");
    }

    #[test]
    fn profile_capabilities() {
        let profile = TestProfile::new();
        assert!(profile.supports_reasoning());
        assert!(profile.supports_streaming());
        assert!(!profile.supports_parallel_tool_calls());
        assert_eq!(profile.context_window_size(), 200_000);
    }

    #[test]
    fn profile_build_system_prompt() {
        let profile = TestProfile::new();
        let env = TestEnv;
        let docs = vec!["README.md contents".into()];
        let prompt = profile.build_system_prompt(&env, &docs);
        assert!(prompt.contains("linux"));
        assert!(prompt.contains("1"));
    }

    #[test]
    fn profile_provider_options_none() {
        let profile = TestProfile::new();
        assert!(profile.provider_options().is_none());
    }

    #[test]
    fn profile_tools_empty_registry() {
        let profile = TestProfile::new();
        assert!(profile.tools().is_empty());
    }
}
