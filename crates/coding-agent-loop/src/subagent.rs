use crate::error::AgentError;
use crate::session::Session;
use crate::tool_registry::RegisteredTool;
use crate::types::Turn;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use unified_llm::types::ToolDefinition;

pub type SessionFactory = Arc<dyn Fn() -> Session + Send + Sync>;

#[derive(Debug, Clone)]
pub struct SubAgentResult {
    pub output: String,
    pub success: bool,
    pub turns_used: usize,
}

pub struct SubAgent {
    id: String,
    depth: usize,
    task: Option<tokio::task::JoinHandle<Result<SubAgentResult, AgentError>>>,
    followup_queue: Arc<Mutex<VecDeque<String>>>,
    abort_flag: Arc<AtomicBool>,
}

impl SubAgent {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn depth(&self) -> usize {
        self.depth
    }
}

pub struct SubAgentManager {
    agents: HashMap<String, SubAgent>,
    max_depth: usize,
}

impl SubAgentManager {
    pub fn new(max_depth: usize) -> Self {
        Self {
            agents: HashMap::new(),
            max_depth,
        }
    }

    pub fn spawn(
        &mut self,
        mut session: Session,
        task_prompt: String,
        depth: usize,
    ) -> Result<String, String> {
        if depth >= self.max_depth {
            return Err(format!(
                "Maximum subagent depth ({}) reached",
                self.max_depth
            ));
        }

        let agent_id = uuid::Uuid::new_v4().to_string();
        let followup_queue = session.followup_queue_handle();
        let abort_flag = session.abort_flag_handle();

        let task = tokio::spawn(async move {
            let result = session.process_input(&task_prompt).await;
            let turns = session.history().turns();
            let turns_used = turns.len();
            let last_text = turns.iter().rev().find_map(|t| {
                if let Turn::Assistant { content, .. } = t {
                    Some(content.clone())
                } else {
                    None
                }
            });
            let success = result.is_ok();
            if let Err(e) = result {
                return Err(e);
            }
            Ok(SubAgentResult {
                output: last_text.unwrap_or_default(),
                success,
                turns_used,
            })
        });

        self.agents.insert(
            agent_id.clone(),
            SubAgent {
                id: agent_id.clone(),
                depth,
                task: Some(task),
                followup_queue,
                abort_flag,
            },
        );

        Ok(agent_id)
    }

    pub fn send_input(&self, agent_id: &str, message: &str) -> Result<(), String> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or_else(|| format!("No agent found with id: {agent_id}"))?;

        agent
            .followup_queue
            .lock()
            .expect("followup queue lock poisoned")
            .push_back(message.to_string());

        Ok(())
    }

    pub async fn wait(&mut self, agent_id: &str) -> Result<SubAgentResult, String> {
        let mut agent = self
            .agents
            .remove(agent_id)
            .ok_or_else(|| format!("No agent found with id: {agent_id}"))?;

        match agent.task.take() {
            Some(join_handle) => match join_handle.await {
                Ok(result) => result.map_err(|e| e.to_string()),
                Err(e) => Err(format!("Agent task panicked: {e}")),
            },
            None => Err(format!("Agent {agent_id} has no running task")),
        }
    }

    pub fn close(&mut self, agent_id: &str) -> Result<(), String> {
        let agent = self
            .agents
            .remove(agent_id)
            .ok_or_else(|| format!("No agent found with id: {agent_id}"))?;

        agent.abort_flag.store(true, Ordering::SeqCst);

        if let Some(join_handle) = agent.task {
            join_handle.abort();
        }

        Ok(())
    }

    pub fn get(&self, agent_id: &str) -> Option<&SubAgent> {
        self.agents.get(agent_id)
    }
}

pub fn make_spawn_agent_tool(
    manager: Arc<tokio::sync::Mutex<SubAgentManager>>,
    session_factory: SessionFactory,
    current_depth: usize,
) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "spawn_agent".into(),
            description: "Spawn a subagent to work on a delegated task".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "task": {
                        "type": "string",
                        "description": "The task description for the subagent"
                    },
                    "working_dir": {
                        "type": "string",
                        "description": "Working directory for the subagent"
                    },
                    "model": {
                        "type": "string",
                        "description": "Model to use for the subagent"
                    },
                    "max_turns": {
                        "type": "integer",
                        "description": "Maximum number of turns for the subagent"
                    }
                },
                "required": ["task"]
            }),
        },
        executor: Arc::new(move |args, _env| {
            let manager = manager.clone();
            let session_factory = session_factory.clone();
            Box::pin(async move {
                let task = args
                    .get("task")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: task".to_string())?;

                // Extract optional max_turns parameter
                #[allow(clippy::cast_possible_truncation)]
                let max_turns = args
                    .get("max_turns")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as usize);

                // Note: working_dir and model require session factory changes to wire through
                let mut session = session_factory();
                if let Some(turns) = max_turns {
                    session.set_max_turns(turns);
                }
                let mut mgr = manager.lock().await;
                mgr.spawn(session, task.to_string(), current_depth)
            })
        }),
    }
}

pub fn make_send_input_tool(
    manager: Arc<tokio::sync::Mutex<SubAgentManager>>,
) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "send_input".into(),
            description: "Send a follow-up message to a running subagent".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The ID of the agent to send input to"
                    },
                    "message": {
                        "type": "string",
                        "description": "The message to send to the agent"
                    }
                },
                "required": ["agent_id", "message"]
            }),
        },
        executor: Arc::new(move |args, _env| {
            let manager = manager.clone();
            Box::pin(async move {
                let agent_id = args
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: agent_id".to_string())?;
                let message = args
                    .get("message")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: message".to_string())?;

                let mgr = manager.lock().await;
                mgr.send_input(agent_id, message)?;
                Ok(format!("Message sent to agent {agent_id}"))
            })
        }),
    }
}

pub fn make_wait_tool(
    manager: Arc<tokio::sync::Mutex<SubAgentManager>>,
) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "wait".into(),
            description: "Wait for a subagent to complete and return its result".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The ID of the agent to wait for"
                    }
                },
                "required": ["agent_id"]
            }),
        },
        executor: Arc::new(move |args, _env| {
            let manager = manager.clone();
            Box::pin(async move {
                let agent_id = args
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: agent_id".to_string())?;

                let mut mgr = manager.lock().await;
                let result = mgr.wait(agent_id).await?;
                Ok(format!(
                    "Agent completed (success: {}, turns: {})\n\n{}",
                    result.success, result.turns_used, result.output
                ))
            })
        }),
    }
}

pub fn make_close_agent_tool(
    manager: Arc<tokio::sync::Mutex<SubAgentManager>>,
) -> RegisteredTool {
    RegisteredTool {
        definition: ToolDefinition {
            name: "close_agent".into(),
            description: "Close a running subagent".into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": {
                        "type": "string",
                        "description": "The ID of the agent to close"
                    }
                },
                "required": ["agent_id"]
            }),
        },
        executor: Arc::new(move |args, _env| {
            let manager = manager.clone();
            Box::pin(async move {
                let agent_id = args
                    .get("agent_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| "Missing required parameter: agent_id".to_string())?;

                let mut mgr = manager.lock().await;
                mgr.close(agent_id)?;
                Ok(format!("Agent {agent_id} closed"))
            })
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SessionConfig;
    use crate::execution_env::*;
    use crate::provider_profile::ProviderProfile;
    use crate::tool_registry::ToolRegistry;
    use async_trait::async_trait;
    use std::sync::atomic::AtomicUsize;
    use unified_llm::client::Client;
    use unified_llm::error::SdkError;
    use unified_llm::provider::{ProviderAdapter, StreamEventStream};
    use unified_llm::types::{FinishReason, Message, Response, Usage};

    // --- Mock LLM Provider ---

    struct MockLlmProvider {
        responses: Vec<Response>,
        call_index: AtomicUsize,
    }

    impl MockLlmProvider {
        fn new(responses: Vec<Response>) -> Self {
            Self {
                responses,
                call_index: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ProviderAdapter for MockLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn complete(
            &self,
            _request: &unified_llm::types::Request,
        ) -> Result<Response, SdkError> {
            let idx = self.call_index.fetch_add(1, Ordering::SeqCst);
            if idx < self.responses.len() {
                Ok(self.responses[idx].clone())
            } else {
                Ok(self.responses[self.responses.len() - 1].clone())
            }
        }

        async fn stream(
            &self,
            _request: &unified_llm::types::Request,
        ) -> Result<StreamEventStream, SdkError> {
            Err(SdkError::Configuration {
                message: "streaming not supported in mock".into(),
            })
        }
    }

    // --- Memory Execution Environment ---

    struct MemoryExecutionEnvironment;

    #[async_trait]
    impl ExecutionEnvironment for MemoryExecutionEnvironment {
        async fn read_file(&self, _path: &str, _offset: Option<usize>, _limit: Option<usize>) -> Result<String, String> {
            Ok(String::new())
        }
        async fn write_file(&self, _path: &str, _content: &str) -> Result<(), String> {
            Ok(())
        }
        async fn file_exists(&self, _path: &str) -> Result<bool, String> {
            Ok(false)
        }
        async fn list_directory(&self, _path: &str, _depth: Option<usize>) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }
        async fn exec_command(
            &self,
            _command: &str,
            _timeout_ms: u64,
            _working_dir: Option<&str>,
            _env_vars: Option<&std::collections::HashMap<String, String>>,
        ) -> Result<ExecResult, String> {
            Ok(ExecResult {
                stdout: "mock output".into(),
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
            Ok(vec![])
        }
        async fn glob(&self, _pattern: &str, _path: Option<&str>) -> Result<Vec<String>, String> {
            Ok(vec![])
        }
        async fn initialize(&self) -> Result<(), String> {
            Ok(())
        }
        async fn cleanup(&self) -> Result<(), String> {
            Ok(())
        }
        fn working_directory(&self) -> &str {
            "/tmp/test"
        }
        fn platform(&self) -> &str {
            "darwin"
        }
        fn os_version(&self) -> String {
            "Darwin 24.0.0".into()
        }
    }

    // --- Test Profile ---

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
            "mock".into()
        }
        fn model(&self) -> String {
            "mock-model".into()
        }
        fn tool_registry(&self) -> &ToolRegistry {
            &self.registry
        }
        fn tool_registry_mut(&mut self) -> &mut ToolRegistry {
            &mut self.registry
        }
        fn build_system_prompt(
            &self,
            _env: &dyn ExecutionEnvironment,
            _env_context: &crate::profiles::EnvContext,
            _project_docs: &[String],
            _user_instructions: Option<&str>,
        ) -> String {
            "You are a test assistant.".into()
        }
        fn tools(&self) -> Vec<ToolDefinition> {
            self.registry.definitions()
        }
        fn provider_options(&self) -> Option<serde_json::Value> {
            None
        }
        fn supports_reasoning(&self) -> bool {
            false
        }
        fn supports_streaming(&self) -> bool {
            false
        }
        fn supports_parallel_tool_calls(&self) -> bool {
            false
        }
        fn context_window_size(&self) -> usize {
            200_000
        }
    }

    // --- Helper functions ---

    fn text_response(text: &str) -> Response {
        Response {
            id: format!("resp_{text}"),
            model: "mock-model".into(),
            provider: "mock".into(),
            message: Message::assistant(text),
            finish_reason: FinishReason::Stop,
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                ..Default::default()
            },
            raw: None,
            warnings: vec![],
            rate_limit: None,
        }
    }

    async fn make_client(provider: Arc<dyn ProviderAdapter>) -> Client {
        let mut providers = HashMap::new();
        providers.insert(provider.name().to_string(), provider);
        Client::new(providers, Some("mock".into()), vec![])
    }

    async fn make_session(responses: Vec<Response>) -> Session {
        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::new());
        let env = Arc::new(MemoryExecutionEnvironment);
        Session::new(client, profile, env, SessionConfig::default())
    }

    // --- Tests ---

    #[test]
    fn manager_creation() {
        let manager = SubAgentManager::new(3);
        assert_eq!(manager.max_depth, 3);
        assert!(manager.agents.is_empty());
    }

    #[tokio::test]
    async fn spawn_creates_agent_and_returns_id() {
        let mut manager = SubAgentManager::new(3);
        let session = make_session(vec![text_response("Hello")]).await;
        let result = manager.spawn(session, "Do something".into(), 0);
        assert!(result.is_ok());
        let agent_id = result.unwrap();
        assert!(!agent_id.is_empty());
        assert!(manager.get(&agent_id).is_some());
        assert_eq!(manager.get(&agent_id).unwrap().depth(), 0);
    }

    #[tokio::test]
    async fn depth_limit_enforced() {
        let mut manager = SubAgentManager::new(2);
        let session = make_session(vec![text_response("Hello")]).await;
        let result = manager.spawn(session, "Do something".into(), 2);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Maximum subagent depth"));
    }

    #[tokio::test]
    async fn close_removes_agent() {
        let mut manager = SubAgentManager::new(3);
        let session = make_session(vec![text_response("Hello")]).await;
        let agent_id = manager.spawn(session, "Do something".into(), 0).unwrap();
        assert!(manager.get(&agent_id).is_some());

        let result = manager.close(&agent_id);
        assert!(result.is_ok());
        assert!(manager.get(&agent_id).is_none());
    }

    #[tokio::test]
    async fn send_input_nonexistent_agent_errors() {
        let manager = SubAgentManager::new(3);
        let result = manager.send_input("nonexistent-id", "hello");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No agent found"));
    }

    #[tokio::test]
    async fn wait_nonexistent_agent_errors() {
        let mut manager = SubAgentManager::new(3);
        let result = manager.wait("nonexistent-id").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No agent found"));
    }

    #[tokio::test]
    async fn wait_returns_result() {
        let mut manager = SubAgentManager::new(3);
        let session =
            make_session(vec![text_response("Task completed successfully")]).await;
        let agent_id = manager.spawn(session, "Do something".into(), 0).unwrap();

        let result = manager.wait(&agent_id).await;
        assert!(result.is_ok());
        let agent_result = result.unwrap();
        assert_eq!(agent_result.output, "Task completed successfully");
        assert!(agent_result.success);
        assert!(agent_result.turns_used > 0);
        assert!(manager.get(&agent_id).is_none());
    }

    #[test]
    fn tool_definitions_correct() {
        let manager = Arc::new(tokio::sync::Mutex::new(SubAgentManager::new(3)));
        let factory: SessionFactory = Arc::new(|| {
            panic!("should not be called");
        });

        let spawn_tool = make_spawn_agent_tool(manager.clone(), factory, 0);
        assert_eq!(spawn_tool.definition.name, "spawn_agent");
        assert!(spawn_tool.definition.parameters["properties"]["task"].is_object());
        let spawn_required = spawn_tool.definition.parameters["required"]
            .as_array()
            .unwrap();
        assert!(spawn_required.contains(&serde_json::json!("task")));

        let send_tool = make_send_input_tool(manager.clone());
        assert_eq!(send_tool.definition.name, "send_input");
        assert!(send_tool.definition.parameters["properties"]["agent_id"].is_object());
        assert!(send_tool.definition.parameters["properties"]["message"].is_object());
        let send_required = send_tool.definition.parameters["required"]
            .as_array()
            .unwrap();
        assert!(send_required.contains(&serde_json::json!("agent_id")));
        assert!(send_required.contains(&serde_json::json!("message")));

        let wait_tool = make_wait_tool(manager.clone());
        assert_eq!(wait_tool.definition.name, "wait");
        assert!(wait_tool.definition.parameters["properties"]["agent_id"].is_object());
        let wait_required = wait_tool.definition.parameters["required"]
            .as_array()
            .unwrap();
        assert!(wait_required.contains(&serde_json::json!("agent_id")));

        let close_tool = make_close_agent_tool(manager);
        assert_eq!(close_tool.definition.name, "close_agent");
        assert!(close_tool.definition.parameters["properties"]["agent_id"].is_object());
        let close_required = close_tool.definition.parameters["required"]
            .as_array()
            .unwrap();
        assert!(close_required.contains(&serde_json::json!("agent_id")));
    }
}
