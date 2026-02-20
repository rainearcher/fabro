use crate::execution_env::ExecutionEnvironment;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use unified_llm::types::ToolDefinition;

pub type ToolExecutor = Arc<
    dyn Fn(
            serde_json::Value,
            Arc<dyn ExecutionEnvironment>,
        ) -> Pin<Box<dyn Future<Output = Result<String, String>> + Send>>
        + Send
        + Sync,
>;

pub struct RegisteredTool {
    pub definition: ToolDefinition,
    pub executor: ToolExecutor,
}

pub struct ToolRegistry {
    tools: HashMap<String, RegisteredTool>,
}

impl ToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: RegisteredTool) {
        self.tools.insert(tool.definition.name.clone(), tool);
    }

    pub fn unregister(&mut self, name: &str) -> Option<RegisteredTool> {
        self.tools.remove(name)
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&RegisteredTool> {
        self.tools.get(name)
    }

    #[must_use]
    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools.values().map(|t| t.definition.clone()).collect()
    }

    #[must_use]
    pub fn names(&self) -> Vec<String> {
        self.tools.keys().cloned().collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool(name: &str) -> RegisteredTool {
        RegisteredTool {
            definition: ToolDefinition {
                name: name.into(),
                description: format!("Tool {name}"),
                parameters: serde_json::json!({"type": "object"}),
            },
            executor: Arc::new(|_args, _env| Box::pin(async { Ok("ok".into()) })),
        }
    }

    #[test]
    fn register_and_get() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("read_file"));

        let tool = registry.get("read_file");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().definition.name, "read_file");
    }

    #[test]
    fn get_missing_returns_none() {
        let registry = ToolRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn unregister_removes_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("read_file"));
        let removed = registry.unregister("read_file");
        assert!(removed.is_some());
        assert!(registry.get("read_file").is_none());
    }

    #[test]
    fn unregister_missing_returns_none() {
        let mut registry = ToolRegistry::new();
        assert!(registry.unregister("nonexistent").is_none());
    }

    #[test]
    fn name_collision_overrides() {
        let mut registry = ToolRegistry::new();
        registry.register(RegisteredTool {
            definition: ToolDefinition {
                name: "tool_a".into(),
                description: "version 1".into(),
                parameters: serde_json::json!({}),
            },
            executor: Arc::new(|_args, _env| Box::pin(async { Ok("v1".into()) })),
        });
        registry.register(RegisteredTool {
            definition: ToolDefinition {
                name: "tool_a".into(),
                description: "version 2".into(),
                parameters: serde_json::json!({}),
            },
            executor: Arc::new(|_args, _env| Box::pin(async { Ok("v2".into()) })),
        });

        let tool = registry.get("tool_a").unwrap();
        assert_eq!(tool.definition.description, "version 2");
    }

    #[test]
    fn definitions_returns_all() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("tool_a"));
        registry.register(make_tool("tool_b"));

        let defs = registry.definitions();
        assert_eq!(defs.len(), 2);
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"tool_a"));
        assert!(names.contains(&"tool_b"));
    }

    #[test]
    fn names_returns_all() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("tool_x"));
        registry.register(make_tool("tool_y"));

        let names = registry.names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"tool_x".to_string()));
        assert!(names.contains(&"tool_y".to_string()));
    }

    #[tokio::test]
    async fn executor_can_be_called() {
        let mut registry = ToolRegistry::new();
        registry.register(make_tool("echo"));

        let tool = registry.get("echo").unwrap();

        use crate::execution_env::*;
        use async_trait::async_trait;

        struct DummyEnv;

        #[async_trait]
        impl ExecutionEnvironment for DummyEnv {
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
                "/tmp"
            }
            fn platform(&self) -> &str {
                "darwin"
            }
            fn os_version(&self) -> String {
                String::new()
            }
        }

        let env: Arc<dyn ExecutionEnvironment> = Arc::new(DummyEnv);
        let result = (tool.executor)(serde_json::json!({}), env).await;
        assert_eq!(result.unwrap(), "ok");
    }

    #[test]
    fn default_creates_empty_registry() {
        let registry = ToolRegistry::default();
        assert!(registry.names().is_empty());
        assert!(registry.definitions().is_empty());
    }
}
