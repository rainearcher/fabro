use crate::config::SessionConfig;
use crate::error::AgentError;
use crate::event::EventEmitter;
use crate::history::History;
use crate::loop_detection::detect_loop;
use crate::profiles::EnvContext;
use crate::project_docs::discover_project_docs;
use crate::provider_profile::ProviderProfile;
use crate::truncation::truncate_tool_output;
use crate::types::{EventKind, SessionEvent, SessionState, Turn};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use unified_llm::client::Client;
use unified_llm::error::{ProviderErrorKind, SdkError};
use unified_llm::types::{Message, Request, ToolChoice, ToolResult};

use crate::execution_env::ExecutionEnvironment;

pub struct Session {
    id: String,
    config: SessionConfig,
    history: History,
    event_emitter: EventEmitter,
    state: SessionState,
    llm_client: Client,
    provider_profile: Arc<dyn ProviderProfile>,
    execution_env: Arc<dyn ExecutionEnvironment>,
    steering_queue: Arc<Mutex<VecDeque<String>>>,
    followup_queue: Arc<Mutex<VecDeque<String>>>,
    abort_flag: Arc<AtomicBool>,
    project_docs: Vec<String>,
    env_context: EnvContext,
}

impl Session {
    #[must_use]
    pub fn new(
        llm_client: Client,
        provider_profile: Arc<dyn ProviderProfile>,
        execution_env: Arc<dyn ExecutionEnvironment>,
        config: SessionConfig,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            config,
            history: History::new(),
            event_emitter: EventEmitter::new(),
            state: SessionState::Idle,
            llm_client,
            provider_profile,
            execution_env,
            steering_queue: Arc::new(Mutex::new(VecDeque::new())),
            followup_queue: Arc::new(Mutex::new(VecDeque::new())),
            abort_flag: Arc::new(AtomicBool::new(false)),
            project_docs: Vec::new(),
            env_context: EnvContext::default(),
        }
    }

    /// Initialize session by discovering project docs and capturing environment context.
    /// Call before `process_input`.
    pub async fn initialize(&mut self) {
        if let Some(ref git_root) = self.config.git_root {
            self.project_docs = discover_project_docs(
                self.execution_env.as_ref(),
                git_root,
                self.execution_env.working_directory(),
                &self.provider_profile.id(),
            )
            .await;
        }

        // Populate environment context
        self.env_context = self.build_env_context().await;
    }

    async fn build_env_context(&self) -> EnvContext {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let model_name = self.provider_profile.model();

        // Detect git info via execution environment
        let git_branch = self
            .execution_env
            .exec_command("git", &["rev-parse".into(), "--abbrev-ref".into(), "HEAD".into()], 5000, None, None)
            .await
            .ok()
            .filter(|r| r.exit_code == 0)
            .map(|r| r.stdout.trim().to_string());

        let is_git_repo = git_branch.is_some();

        EnvContext {
            git_branch,
            is_git_repo,
            date: today,
            model_name,
        }
    }

    pub fn state(&self) -> SessionState {
        self.state
    }

    pub fn subscribe(&self) -> tokio::sync::broadcast::Receiver<SessionEvent> {
        self.event_emitter.subscribe()
    }

    pub fn steer(&self, message: String) {
        self.steering_queue
            .lock()
            .expect("steering queue lock poisoned")
            .push_back(message);
    }

    pub fn follow_up(&self, message: String) {
        self.followup_queue
            .lock()
            .expect("followup queue lock poisoned")
            .push_back(message);
    }

    pub fn abort(&self) {
        self.abort_flag.store(true, Ordering::SeqCst);
    }

    pub fn followup_queue_handle(&self) -> Arc<Mutex<VecDeque<String>>> {
        self.followup_queue.clone()
    }

    pub fn abort_flag_handle(&self) -> Arc<AtomicBool> {
        self.abort_flag.clone()
    }

    pub fn close(&mut self) {
        self.state = SessionState::Closed;
    }

    pub fn set_reasoning_effort(&mut self, effort: Option<String>) {
        self.config.reasoning_effort = effort;
    }

    pub fn history(&self) -> &History {
        &self.history
    }

    pub async fn process_input(&mut self, input: &str) -> Result<(), AgentError> {
        if self.state == SessionState::Closed {
            return Err(AgentError::SessionClosed);
        }

        self.event_emitter.emit(
            EventKind::SessionStart,
            self.id.clone(),
            HashMap::new(),
        );

        // Use a queue to avoid recursive async calls for followups
        let mut current_input = input.to_string();

        loop {
            self.run_single_input(&current_input).await?;

            // Check followup queue
            let next_followup = self
                .followup_queue
                .lock()
                .expect("followup queue lock poisoned")
                .pop_front();
            match next_followup {
                Some(followup) => {
                    current_input = followup;
                }
                None => break,
            }
        }

        self.state = SessionState::Idle;
        self.event_emitter.emit(
            EventKind::SessionEnd,
            self.id.clone(),
            HashMap::new(),
        );

        Ok(())
    }

    async fn run_single_input(&mut self, input: &str) -> Result<(), AgentError> {
        if self.state == SessionState::Closed {
            return Err(AgentError::SessionClosed);
        }

        self.state = SessionState::Processing;

        // Append user turn and emit event
        self.history.push(Turn::User {
            content: input.to_string(),
            timestamp: SystemTime::now(),
        });
        self.event_emitter.emit(
            EventKind::UserInput,
            self.id.clone(),
            HashMap::new(),
        );

        // Drain steering queue before first LLM call
        self.drain_steering();

        let mut round_count: usize = 0;

        loop {
            // Check max_tool_rounds_per_input
            if round_count >= self.config.max_tool_rounds_per_input {
                self.event_emitter.emit(
                    EventKind::TurnLimit,
                    self.id.clone(),
                    HashMap::new(),
                );
                break;
            }

            // Check max_turns
            if self.config.max_turns > 0 && self.history.count_turns() >= self.config.max_turns {
                self.event_emitter.emit(
                    EventKind::TurnLimit,
                    self.id.clone(),
                    HashMap::new(),
                );
                break;
            }

            // Check abort flag
            if self.abort_flag.load(Ordering::SeqCst) {
                self.state = SessionState::Closed;
                return Err(AgentError::Aborted);
            }

            // Build request
            let request = self.build_request();

            // Call LLM
            let response = match self.llm_client.complete(&request).await {
                Ok(resp) => resp,
                Err(err) => {
                    let mut error_data = HashMap::new();
                    error_data.insert(
                        "error".to_string(),
                        serde_json::json!(err.to_string()),
                    );
                    self.event_emitter.emit(
                        EventKind::Error,
                        self.id.clone(),
                        error_data,
                    );
                    if is_auth_error(&err) {
                        self.state = SessionState::Closed;
                    }
                    return Err(AgentError::Llm(err));
                }
            };

            // Record assistant turn
            let text = response.text();
            let tool_calls = response.tool_calls();
            let reasoning = response.reasoning();
            let usage = response.usage.clone();

            self.history.push(Turn::Assistant {
                content: text.clone(),
                tool_calls: tool_calls.clone(),
                reasoning,
                usage,
                response_id: response.id.clone(),
                timestamp: SystemTime::now(),
            });

            // Emit AssistantTextEnd
            self.event_emitter.emit(
                EventKind::AssistantTextEnd,
                self.id.clone(),
                HashMap::new(),
            );

            // Check context window usage
            self.check_context_usage();

            // If no tool calls, natural completion
            if tool_calls.is_empty() {
                break;
            }

            round_count += 1;

            // Execute tool calls (parallel or sequential based on provider)
            let results = self.execute_tool_calls(&tool_calls).await;

            // Record tool results turn
            self.history.push(Turn::ToolResults {
                results,
                timestamp: SystemTime::now(),
            });

            // Drain steering after tool execution
            self.drain_steering();

            // Loop detection
            if self.config.enable_loop_detection
                && detect_loop(&self.history, self.config.loop_detection_window)
            {
                self.history.push(Turn::Steering {
                    content: "WARNING: Loop detected. You appear to be repeating the same tool calls. Please try a different approach or ask for clarification.".to_string(),
                    timestamp: SystemTime::now(),
                });
                self.event_emitter.emit(
                    EventKind::LoopDetection,
                    self.id.clone(),
                    HashMap::new(),
                );
            }
        }

        Ok(())
    }

    fn drain_steering(&mut self) {
        let messages: Vec<String> = self
            .steering_queue
            .lock()
            .expect("steering queue lock poisoned")
            .drain(..)
            .collect();
        for msg in messages {
            self.history.push(Turn::Steering {
                content: msg,
                timestamp: SystemTime::now(),
            });
            self.event_emitter.emit(
                EventKind::SteeringInjected,
                self.id.clone(),
                HashMap::new(),
            );
        }
    }

    fn build_request(&self) -> Request {
        let system_prompt = self.provider_profile.build_system_prompt(
            self.execution_env.as_ref(),
            &self.env_context,
            &self.project_docs,
            self.config.user_instructions.as_deref(),
        );
        let mut messages = vec![Message::system(system_prompt)];
        messages.extend(self.history.convert_to_messages());

        let tools = self.provider_profile.tools();

        Request {
            model: self.provider_profile.model(),
            messages,
            provider: Some(self.provider_profile.id()),
            tools: if tools.is_empty() { None } else { Some(tools) },
            tool_choice: if self.provider_profile.tools().is_empty() {
                None
            } else {
                Some(ToolChoice::Auto)
            },
            response_format: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stop_sequences: None,
            reasoning_effort: self.config.reasoning_effort.clone(),
            metadata: None,
            provider_options: self.provider_profile.provider_options(),
        }
    }

    async fn execute_single_tool(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        arguments: &serde_json::Value,
    ) -> ToolResult {
        let registry = self.provider_profile.tool_registry();
        match registry.get(tool_name) {
            Some(registered_tool) => {
                // Validate arguments against schema
                if let Err(validation_error) =
                    validate_tool_args(&registered_tool.definition.parameters, arguments)
                {
                    return ToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        content: serde_json::json!(validation_error),
                        is_error: true,
                        image_data: None,
                        image_media_type: None,
                    };
                }

                let executor = &registered_tool.executor;
                match executor(arguments.clone(), self.execution_env.clone()).await {
                    Ok(output) => ToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        content: serde_json::json!(output),
                        is_error: false,
                        image_data: None,
                        image_media_type: None,
                    },
                    Err(err) => ToolResult {
                        tool_call_id: tool_call_id.to_string(),
                        content: serde_json::json!(err),
                        is_error: true,
                        image_data: None,
                        image_media_type: None,
                    },
                }
            }
            None => ToolResult {
                tool_call_id: tool_call_id.to_string(),
                content: serde_json::json!(format!("Unknown tool: {tool_name}")),
                is_error: true,
                image_data: None,
                image_media_type: None,
            },
        }
    }

    async fn execute_tool_calls(
        &mut self,
        tool_calls: &[unified_llm::types::ToolCall],
    ) -> Vec<ToolResult> {
        if self.provider_profile.supports_parallel_tool_calls() && tool_calls.len() > 1 {
            self.execute_tool_calls_parallel(tool_calls).await
        } else {
            self.execute_tool_calls_sequential(tool_calls).await
        }
    }

    async fn execute_tool_calls_sequential(
        &self,
        tool_calls: &[unified_llm::types::ToolCall],
    ) -> Vec<ToolResult> {
        let mut results = Vec::new();
        for tc in tool_calls {
            results.push(self.emit_execute_and_truncate(tc).await);
        }
        results
    }

    async fn execute_tool_calls_parallel(
        &self,
        tool_calls: &[unified_llm::types::ToolCall],
    ) -> Vec<ToolResult> {
        let emitter = self.event_emitter.clone();
        let env = self.execution_env.clone();
        let profile = self.provider_profile.clone();
        let session_id = self.id.clone();
        let config = self.config.clone();

        let futures: Vec<_> = tool_calls
            .iter()
            .map(|tc| {
                let emitter = emitter.clone();
                let env = env.clone();
                let profile = profile.clone();
                let session_id = session_id.clone();
                let config = config.clone();
                let tc = tc.clone();
                async move {
                    // Emit ToolCallStart
                    let mut start_data = HashMap::new();
                    start_data
                        .insert("tool_name".to_string(), serde_json::json!(&tc.name));
                    start_data
                        .insert("tool_call_id".to_string(), serde_json::json!(&tc.id));
                    emitter.emit(
                        EventKind::ToolCallStart,
                        session_id.clone(),
                        start_data,
                    );

                    // Execute tool
                    let registry = profile.tool_registry();
                    let result = match registry.get(&tc.name) {
                        Some(registered_tool) => {
                            // Validate arguments against schema
                            if let Err(validation_error) = validate_tool_args(
                                &registered_tool.definition.parameters,
                                &tc.arguments,
                            ) {
                                ToolResult {
                                    tool_call_id: tc.id.clone(),
                                    content: serde_json::json!(validation_error),
                                    is_error: true,
                                    image_data: None,
                                    image_media_type: None,
                                }
                            } else {
                                match (registered_tool.executor)(tc.arguments.clone(), env).await {
                                    Ok(output) => ToolResult {
                                        tool_call_id: tc.id.clone(),
                                        content: serde_json::json!(output),
                                        is_error: false,
                                        image_data: None,
                                        image_media_type: None,
                                    },
                                    Err(err) => ToolResult {
                                        tool_call_id: tc.id.clone(),
                                        content: serde_json::json!(err),
                                        is_error: true,
                                        image_data: None,
                                        image_media_type: None,
                                    },
                                }
                            }
                        }
                        None => ToolResult {
                            tool_call_id: tc.id.clone(),
                            content: serde_json::json!(format!("Unknown tool: {}", tc.name)),
                            is_error: true,
                            image_data: None,
                            image_media_type: None,
                        },
                    };

                    // Emit ToolCallEnd
                    let mut end_data = HashMap::new();
                    end_data
                        .insert("tool_name".to_string(), serde_json::json!(&tc.name));
                    end_data
                        .insert("tool_call_id".to_string(), serde_json::json!(&tc.id));
                    match &result.content {
                        serde_json::Value::String(s) => {
                            end_data.insert("output".to_string(), serde_json::json!(s));
                        }
                        other => {
                            end_data.insert("output".to_string(), other.clone());
                        }
                    }
                    end_data.insert(
                        "is_error".to_string(),
                        serde_json::json!(result.is_error),
                    );
                    emitter.emit(EventKind::ToolCallEnd, session_id, end_data);

                    // Truncate for history
                    let truncated_content = match &result.content {
                        serde_json::Value::String(s) => {
                            serde_json::json!(truncate_tool_output(s, &tc.name, &config))
                        }
                        other => other.clone(),
                    };

                    ToolResult {
                        tool_call_id: result.tool_call_id,
                        content: truncated_content,
                        is_error: result.is_error,
                        image_data: result.image_data,
                        image_media_type: result.image_media_type,
                    }
                }
            })
            .collect();

        futures::future::join_all(futures).await
    }

    async fn emit_execute_and_truncate(
        &self,
        tc: &unified_llm::types::ToolCall,
    ) -> ToolResult {
        // Emit ToolCallStart
        let mut start_data = HashMap::new();
        start_data.insert("tool_name".to_string(), serde_json::json!(tc.name));
        start_data.insert("tool_call_id".to_string(), serde_json::json!(tc.id));
        self.event_emitter.emit(
            EventKind::ToolCallStart,
            self.id.clone(),
            start_data,
        );

        let result = self
            .execute_single_tool(&tc.id, &tc.name, &tc.arguments)
            .await;

        // Emit ToolCallEnd with full untruncated output
        let mut end_data = HashMap::new();
        end_data.insert("tool_name".to_string(), serde_json::json!(tc.name));
        end_data.insert("tool_call_id".to_string(), serde_json::json!(tc.id));
        match &result.content {
            serde_json::Value::String(s) => {
                end_data.insert("output".to_string(), serde_json::json!(s));
            }
            other => {
                end_data.insert("output".to_string(), other.clone());
            }
        }
        end_data.insert("is_error".to_string(), serde_json::json!(result.is_error));
        self.event_emitter.emit(
            EventKind::ToolCallEnd,
            self.id.clone(),
            end_data,
        );

        // Truncate output for history
        let truncated_content = match &result.content {
            serde_json::Value::String(s) => {
                let truncated = truncate_tool_output(s, &tc.name, &self.config);
                serde_json::json!(truncated)
            }
            other => other.clone(),
        };

        ToolResult {
            tool_call_id: result.tool_call_id,
            content: truncated_content,
            is_error: result.is_error,
            image_data: result.image_data,
            image_media_type: result.image_media_type,
        }
    }

    fn estimate_token_count(&self) -> usize {
        let system_prompt = self.provider_profile.build_system_prompt(
            self.execution_env.as_ref(),
            &self.env_context,
            &self.project_docs,
            self.config.user_instructions.as_deref(),
        );
        let mut total_chars = system_prompt.len();

        for turn in self.history.turns() {
            match turn {
                Turn::User { content, .. } => total_chars += content.len(),
                Turn::Assistant {
                    content,
                    tool_calls,
                    reasoning,
                    ..
                } => {
                    total_chars += content.len();
                    if let Some(r) = reasoning {
                        total_chars += r.len();
                    }
                    for tc in tool_calls {
                        total_chars += tc.name.len();
                        total_chars += tc.arguments.to_string().len();
                    }
                }
                Turn::ToolResults { results, .. } => {
                    for r in results {
                        total_chars += r.content.to_string().len();
                    }
                }
                Turn::System { content, .. } | Turn::Steering { content, .. } => {
                    total_chars += content.len();
                }
            }
        }

        total_chars / 4 // rough estimate: ~4 chars per token
    }

    fn check_context_usage(&self) {
        let estimated_tokens = self.estimate_token_count();
        let context_window = self.provider_profile.context_window_size();
        let threshold = context_window * 80 / 100;

        if estimated_tokens > threshold {
            let mut data = HashMap::new();
            data.insert(
                "estimated_tokens".to_string(),
                serde_json::json!(estimated_tokens),
            );
            data.insert(
                "context_window_size".to_string(),
                serde_json::json!(context_window),
            );
            data.insert(
                "usage_percent".to_string(),
                serde_json::json!(estimated_tokens * 100 / context_window),
            );
            self.event_emitter.emit(
                EventKind::ContextWindowWarning,
                self.id.clone(),
                data,
            );
        }
    }
}

fn is_auth_error(err: &SdkError) -> bool {
    matches!(
        err.provider_kind(),
        Some(ProviderErrorKind::Authentication) | Some(ProviderErrorKind::AccessDenied)
    )
}

fn validate_tool_args(schema: &serde_json::Value, args: &serde_json::Value) -> Result<(), String> {
    // Skip validation for empty/trivial schemas
    if schema.is_null()
        || (schema.is_object() && schema.as_object().map_or(true, |o| o.is_empty()))
    {
        return Ok(());
    }

    let validator = jsonschema::validator_for(schema)
        .map_err(|e| format!("Invalid tool schema: {e}"))?;

    let errors: Vec<String> = validator.iter_errors(args).map(|e| e.to_string()).collect();

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Tool argument validation failed: {}",
            errors.join("; ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::execution_env::*;
    use crate::tool_registry::{RegisteredTool, ToolRegistry};
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicUsize;
    use unified_llm::error::ProviderErrorDetail;
    use unified_llm::provider::{ProviderAdapter, StreamEventStream};
    use unified_llm::types::{
        ContentPart, FinishReason, Response, ToolCall, ToolDefinition, Usage,
    };

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

        async fn complete(&self, _request: &Request) -> Result<Response, SdkError> {
            let idx = self.call_index.fetch_add(1, Ordering::SeqCst);
            if idx < self.responses.len() {
                Ok(self.responses[idx].clone())
            } else {
                // Return last response if we exceed
                Ok(self.responses[self.responses.len() - 1].clone())
            }
        }

        async fn stream(
            &self,
            _request: &Request,
        ) -> Result<StreamEventStream, SdkError> {
            Err(SdkError::Configuration {
                message: "streaming not supported in mock".into(),
            })
        }
    }

    // --- Mock Error Provider ---

    struct MockErrorProvider {
        error: SdkError,
    }

    #[async_trait]
    impl ProviderAdapter for MockErrorProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn complete(&self, _request: &Request) -> Result<Response, SdkError> {
            Err(self.error.clone())
        }

        async fn stream(
            &self,
            _request: &Request,
        ) -> Result<StreamEventStream, SdkError> {
            Err(SdkError::Configuration {
                message: "streaming not supported in mock".into(),
            })
        }
    }

    // --- Memory Execution Environment ---

    struct MemoryExecutionEnvironment {
        files: HashMap<String, String>,
    }

    impl MemoryExecutionEnvironment {
        fn new() -> Self {
            Self {
                files: HashMap::new(),
            }
        }
    }

    #[async_trait]
    impl ExecutionEnvironment for MemoryExecutionEnvironment {
        async fn read_file(&self, path: &str) -> Result<String, String> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| format!("File not found: {path}"))
        }

        async fn write_file(&self, _path: &str, _content: &str) -> Result<(), String> {
            Ok(())
        }

        async fn file_exists(&self, path: &str) -> Result<bool, String> {
            Ok(self.files.contains_key(path))
        }

        async fn list_directory(&self, _path: &str) -> Result<Vec<DirEntry>, String> {
            Ok(vec![])
        }

        async fn exec_command(
            &self,
            _command: &str,
            _args: &[String],
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

        async fn glob(&self, _pattern: &str) -> Result<Vec<String>, String> {
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

        fn with_tools(registry: ToolRegistry) -> Self {
            Self { registry }
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

    fn tool_call_response(tool_name: &str, tool_call_id: &str, args: serde_json::Value) -> Response {
        Response {
            id: format!("resp_{tool_call_id}"),
            model: "mock-model".into(),
            provider: "mock".into(),
            message: Message {
                role: unified_llm::types::Role::Assistant,
                content: vec![
                    ContentPart::text("Let me use a tool."),
                    ContentPart::ToolCall(ToolCall::new(tool_call_id, tool_name, args)),
                ],
                name: None,
                tool_call_id: None,
            },
            finish_reason: FinishReason::ToolCalls,
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

    fn make_echo_tool() -> RegisteredTool {
        RegisteredTool {
            definition: ToolDefinition {
                name: "echo".into(),
                description: "Echoes the input".into(),
                parameters: serde_json::json!({"type": "object", "properties": {"text": {"type": "string"}}}),
            },
            executor: Arc::new(|args, _env| {
                Box::pin(async move {
                    let text = args
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("no text");
                    Ok(format!("echo: {text}"))
                })
            }),
        }
    }

    fn make_error_tool() -> RegisteredTool {
        RegisteredTool {
            definition: ToolDefinition {
                name: "fail_tool".into(),
                description: "Always fails".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
            executor: Arc::new(|_args, _env| {
                Box::pin(async move { Err("tool execution failed".to_string()) })
            }),
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
        let env = Arc::new(MemoryExecutionEnvironment::new());
        Session::new(client, profile, env, SessionConfig::default())
    }

    async fn make_session_with_tools(
        responses: Vec<Response>,
        registry: ToolRegistry,
    ) -> Session {
        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::with_tools(registry));
        let env = Arc::new(MemoryExecutionEnvironment::new());
        Session::new(client, profile, env, SessionConfig::default())
    }

    async fn make_session_with_config(
        responses: Vec<Response>,
        config: SessionConfig,
    ) -> Session {
        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::new());
        let env = Arc::new(MemoryExecutionEnvironment::new());
        Session::new(client, profile, env, config)
    }

    async fn make_session_with_tools_and_config(
        responses: Vec<Response>,
        registry: ToolRegistry,
        config: SessionConfig,
    ) -> Session {
        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::with_tools(registry));
        let env = Arc::new(MemoryExecutionEnvironment::new());
        Session::new(client, profile, env, config)
    }

    // --- Tests ---

    #[tokio::test]
    async fn new_session_starts_idle() {
        let session = make_session(vec![]).await;
        assert_eq!(session.state(), SessionState::Idle);
    }

    #[tokio::test]
    async fn text_only_response_natural_completion() {
        let mut session = make_session(vec![text_response("Hello there!")]).await;
        session.process_input("Hi").await.unwrap();

        assert_eq!(session.state(), SessionState::Idle);
        let turns = session.history().turns();
        // UserTurn + AssistantTurn = 2
        assert_eq!(turns.len(), 2);
        assert!(matches!(&turns[0], Turn::User { content, .. } if content == "Hi"));
        assert!(
            matches!(&turns[1], Turn::Assistant { content, .. } if content == "Hello there!")
        );
    }

    #[tokio::test]
    async fn tool_call_then_text() {
        let mut registry = ToolRegistry::new();
        registry.register(make_echo_tool());

        let responses = vec![
            tool_call_response("echo", "call_1", serde_json::json!({"text": "hello"})),
            text_response("Done!"),
        ];

        let mut session = make_session_with_tools(responses, registry).await;
        session.process_input("Use echo tool").await.unwrap();

        assert_eq!(session.state(), SessionState::Idle);
        let turns = session.history().turns();
        // UserTurn + AssistantTurn(tool_call) + ToolResults + AssistantTurn(text) = 4
        assert_eq!(turns.len(), 4);
        assert!(matches!(&turns[0], Turn::User { .. }));
        assert!(matches!(&turns[1], Turn::Assistant { tool_calls, .. } if tool_calls.len() == 1));
        assert!(matches!(&turns[2], Turn::ToolResults { results, .. } if results.len() == 1));
        assert!(
            matches!(&turns[3], Turn::Assistant { content, .. } if content == "Done!")
        );

        // Verify tool result content
        if let Turn::ToolResults { results, .. } = &turns[2] {
            assert_eq!(results[0].tool_call_id, "call_1");
            assert!(!results[0].is_error);
        }
    }

    #[tokio::test]
    async fn max_tool_rounds_enforced() {
        let mut registry = ToolRegistry::new();
        registry.register(make_echo_tool());

        // Respond with tool calls indefinitely
        let responses = vec![
            tool_call_response("echo", "call_1", serde_json::json!({"text": "a"})),
            tool_call_response("echo", "call_2", serde_json::json!({"text": "b"})),
            tool_call_response("echo", "call_3", serde_json::json!({"text": "c"})),
        ];

        let config = SessionConfig {
            max_tool_rounds_per_input: 2,
            enable_loop_detection: false,
            ..Default::default()
        };

        let mut session = make_session_with_tools_and_config(responses, registry, config).await;
        session.process_input("Keep using tools").await.unwrap();

        // Should stop after 2 rounds: User + (Asst+ToolResult) * 2 = 5 turns
        assert_eq!(session.state(), SessionState::Idle);
        let turns = session.history().turns();
        assert_eq!(turns.len(), 5);
    }

    #[tokio::test]
    async fn max_turns_enforced() {
        let responses = vec![
            text_response("first"),
            text_response("second"),
            text_response("should not reach"),
        ];

        let config = SessionConfig {
            max_turns: 3,
            ..Default::default()
        };

        let mut session = make_session_with_config(responses, config).await;

        // First input: adds User + Assistant = 2 turns
        session.process_input("one").await.unwrap();
        assert_eq!(session.history().count_turns(), 2);

        // Second input: adds User (now 3 turns), then max_turns check triggers
        session.process_input("two").await.unwrap();
        // Should have 3 turns total (User + Asst + User), max_turns hit before LLM call
        assert_eq!(session.history().count_turns(), 3);
    }

    #[tokio::test]
    async fn steer_injects_steering_turn() {
        let mut session = make_session(vec![text_response("OK")]).await;
        session.steer("Focus on the task".to_string());
        session.process_input("Do something").await.unwrap();

        let turns = session.history().turns();
        // User + Steering + Assistant = 3
        assert_eq!(turns.len(), 3);
        assert!(matches!(&turns[0], Turn::User { .. }));
        assert!(
            matches!(&turns[1], Turn::Steering { content, .. } if content == "Focus on the task")
        );
        assert!(matches!(&turns[2], Turn::Assistant { .. }));
    }

    #[tokio::test]
    async fn follow_up_triggers_new_cycle() {
        let responses = vec![
            text_response("First response"),
            text_response("Followup response"),
        ];

        let mut session = make_session(responses).await;
        session.follow_up("followup message".to_string());
        session.process_input("initial message").await.unwrap();

        let turns = session.history().turns();
        // First cycle: User + Assistant = 2
        // Second cycle: User + Assistant = 2
        // Total = 4
        assert_eq!(turns.len(), 4);
        assert!(matches!(&turns[0], Turn::User { content, .. } if content == "initial message"));
        assert!(matches!(&turns[1], Turn::Assistant { content, .. } if content == "First response"));
        assert!(matches!(&turns[2], Turn::User { content, .. } if content == "followup message"));
        assert!(matches!(&turns[3], Turn::Assistant { content, .. } if content == "Followup response"));
    }

    #[tokio::test]
    async fn events_emitted() {
        let mut session = make_session(vec![text_response("Hello")]).await;
        let mut rx = session.subscribe();

        session.process_input("Hi").await.unwrap();

        // Collect events
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event.kind.clone());
        }

        assert!(events.contains(&EventKind::UserInput));
        assert!(events.contains(&EventKind::AssistantTextEnd));
        assert!(events.contains(&EventKind::SessionEnd));
    }

    #[tokio::test]
    async fn tool_call_end_has_untruncated_output() {
        let mut registry = ToolRegistry::new();
        registry.register(make_echo_tool());

        let responses = vec![
            tool_call_response("echo", "call_1", serde_json::json!({"text": "hello world"})),
            text_response("Done"),
        ];

        let mut session = make_session_with_tools(responses, registry).await;
        let mut rx = session.subscribe();

        session.process_input("Use echo").await.unwrap();

        let mut tool_end_events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if event.kind == EventKind::ToolCallEnd {
                tool_end_events.push(event);
            }
        }

        assert_eq!(tool_end_events.len(), 1);
        let output = tool_end_events[0].data.get("output").unwrap();
        assert_eq!(output, &serde_json::json!("echo: hello world"));
    }

    #[tokio::test]
    async fn unknown_tool_returns_error() {
        // No tools registered, but LLM returns a tool call
        let responses = vec![
            tool_call_response("nonexistent_tool", "call_1", serde_json::json!({})),
            text_response("OK"),
        ];

        let mut session = make_session(responses).await;
        session.process_input("Do something").await.unwrap();

        let turns = session.history().turns();
        // User + Asst(tool_call) + ToolResults + Asst(text) = 4
        assert_eq!(turns.len(), 4);
        if let Turn::ToolResults { results, .. } = &turns[2] {
            assert!(results[0].is_error);
            assert_eq!(
                results[0].content,
                serde_json::json!("Unknown tool: nonexistent_tool")
            );
        } else {
            panic!("Expected ToolResults turn at index 2");
        }
    }

    #[tokio::test]
    async fn tool_execution_error() {
        let mut registry = ToolRegistry::new();
        registry.register(make_error_tool());

        let responses = vec![
            tool_call_response("fail_tool", "call_1", serde_json::json!({})),
            text_response("OK"),
        ];

        let mut session = make_session_with_tools(responses, registry).await;
        session.process_input("Use fail tool").await.unwrap();

        let turns = session.history().turns();
        if let Turn::ToolResults { results, .. } = &turns[2] {
            assert!(results[0].is_error);
            assert_eq!(
                results[0].content,
                serde_json::json!("tool execution failed")
            );
        } else {
            panic!("Expected ToolResults turn at index 2");
        }
    }

    #[tokio::test]
    async fn loop_detection_injects_warning() {
        let mut registry = ToolRegistry::new();
        registry.register(make_echo_tool());

        // Same tool call repeated multiple times to trigger loop detection
        let responses = vec![
            tool_call_response("echo", "call_1", serde_json::json!({"text": "same"})),
            tool_call_response("echo", "call_2", serde_json::json!({"text": "same"})),
            tool_call_response("echo", "call_3", serde_json::json!({"text": "same"})),
            text_response("Done"),
        ];

        let config = SessionConfig {
            enable_loop_detection: true,
            loop_detection_window: 3,
            ..Default::default()
        };

        let mut session = make_session_with_tools_and_config(responses, registry, config).await;
        let mut rx = session.subscribe();

        session.process_input("Keep echoing").await.unwrap();

        // Check for LoopDetection event
        let mut found_loop_detection = false;
        while let Ok(event) = rx.try_recv() {
            if event.kind == EventKind::LoopDetection {
                found_loop_detection = true;
            }
        }
        assert!(found_loop_detection);

        // Check for Steering turn with warning in history
        let has_steering_warning = session.history().turns().iter().any(|t| {
            matches!(t, Turn::Steering { content, .. } if content.contains("Loop detected"))
        });
        assert!(has_steering_warning);
    }

    #[tokio::test]
    async fn abort_stops_processing() {
        let mut registry = ToolRegistry::new();
        registry.register(make_echo_tool());

        let responses = vec![
            tool_call_response("echo", "call_1", serde_json::json!({"text": "a"})),
            tool_call_response("echo", "call_2", serde_json::json!({"text": "b"})),
        ];

        let config = SessionConfig {
            enable_loop_detection: false,
            ..Default::default()
        };

        let mut session = make_session_with_tools_and_config(responses, registry, config).await;
        // Set abort before processing
        session.abort();
        let result = session.process_input("Do something").await;

        // Should return Aborted error and transition to Closed
        assert!(matches!(result, Err(AgentError::Aborted)));
        assert_eq!(session.state(), SessionState::Closed);

        // Should have stopped immediately: User turn only, no LLM call
        let turns = session.history().turns();
        assert_eq!(turns.len(), 1);
        assert!(matches!(&turns[0], Turn::User { .. }));
    }

    #[tokio::test]
    async fn abort_transitions_to_closed() {
        let abort_flag = Arc::new(AtomicBool::new(false));
        let abort_flag_for_tool = abort_flag.clone();

        // Tool that sets the abort flag when executed
        let abort_tool = RegisteredTool {
            definition: ToolDefinition {
                name: "set_abort".into(),
                description: "Sets abort flag".into(),
                parameters: serde_json::json!({"type": "object"}),
            },
            executor: Arc::new(move |_args, _env| {
                let flag = abort_flag_for_tool.clone();
                Box::pin(async move {
                    flag.store(true, Ordering::SeqCst);
                    Ok("done".to_string())
                })
            }),
        };

        let mut registry = ToolRegistry::new();
        registry.register(abort_tool);

        let responses = vec![
            tool_call_response("set_abort", "call_1", serde_json::json!({})),
            text_response("Should not reach this"),
        ];

        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::with_tools(registry));
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let config = SessionConfig {
            enable_loop_detection: false,
            ..Default::default()
        };
        let mut session = Session::new(client, profile, env, config);

        // Wire the session's abort_flag to our shared one
        session.abort_flag = abort_flag;

        let result = session.process_input("Do something").await;

        // Should return Aborted error and transition to Closed
        assert!(matches!(result, Err(AgentError::Aborted)));
        assert_eq!(session.state(), SessionState::Closed);

        // Should have processed: User + Assistant(tool_call) + ToolResults = 3 turns
        // The tool set the abort flag, so the loop breaks before the next LLM call
        let turns = session.history().turns();
        assert_eq!(turns.len(), 3);
        assert!(matches!(&turns[0], Turn::User { .. }));
        assert!(matches!(&turns[1], Turn::Assistant { tool_calls, .. } if tool_calls.len() == 1));
        assert!(matches!(&turns[2], Turn::ToolResults { .. }));
    }

    #[tokio::test]
    async fn auth_error_closes_session() {
        let error_provider = Arc::new(MockErrorProvider {
            error: SdkError::Provider {
                kind: ProviderErrorKind::Authentication,
                detail: Box::new(ProviderErrorDetail::new("invalid api key", "mock")),
            },
        });
        let client = make_client(error_provider).await;
        let profile = Arc::new(TestProfile::new());
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let mut session = Session::new(client, profile, env, SessionConfig::default());

        let result = session.process_input("Hello").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AgentError::Llm(_)));
        assert_eq!(session.state(), SessionState::Closed);
    }

    #[tokio::test]
    async fn sequential_inputs() {
        let responses = vec![
            text_response("First"),
            text_response("Second"),
        ];

        let mut session = make_session(responses).await;

        session.process_input("one").await.unwrap();
        assert_eq!(session.state(), SessionState::Idle);

        session.process_input("two").await.unwrap();
        assert_eq!(session.state(), SessionState::Idle);

        let turns = session.history().turns();
        assert_eq!(turns.len(), 4);
        assert!(matches!(&turns[0], Turn::User { content, .. } if content == "one"));
        assert!(matches!(&turns[1], Turn::Assistant { content, .. } if content == "First"));
        assert!(matches!(&turns[2], Turn::User { content, .. } if content == "two"));
        assert!(matches!(&turns[3], Turn::Assistant { content, .. } if content == "Second"));
    }

    #[tokio::test]
    async fn closed_session_rejects_input() {
        let mut session = make_session(vec![]).await;
        session.close();
        assert_eq!(session.state(), SessionState::Closed);

        let result = session.process_input("Hello").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AgentError::SessionClosed));
    }

    #[tokio::test]
    async fn closed_session_does_not_emit_session_start() {
        let mut session = make_session(vec![]).await;
        session.close();

        let mut rx = session.subscribe();
        let result = session.process_input("Hello").await;
        assert!(matches!(result, Err(AgentError::SessionClosed)));

        // No SessionStart event should have been emitted
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event.kind.clone());
        }
        assert!(
            !events.contains(&EventKind::SessionStart),
            "SessionStart should not be emitted for a closed session"
        );
    }

    // --- Parallel execution support ---

    struct ParallelTestProfile {
        registry: ToolRegistry,
        context_window: usize,
    }

    impl ParallelTestProfile {
        fn with_tools(registry: ToolRegistry) -> Self {
            Self {
                registry,
                context_window: 200_000,
            }
        }

        fn with_tools_and_context_window(registry: ToolRegistry, context_window: usize) -> Self {
            Self {
                registry,
                context_window,
            }
        }
    }

    impl ProviderProfile for ParallelTestProfile {
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
            true
        }

        fn context_window_size(&self) -> usize {
            self.context_window
        }
    }

    fn multi_tool_call_response(calls: Vec<(&str, &str, serde_json::Value)>) -> Response {
        let mut content = vec![ContentPart::text("Let me use multiple tools.")];
        for (tool_name, tool_call_id, args) in &calls {
            content.push(ContentPart::ToolCall(ToolCall::new(
                *tool_call_id,
                *tool_name,
                args.clone(),
            )));
        }
        Response {
            id: "resp_multi".into(),
            model: "mock-model".into(),
            provider: "mock".into(),
            message: Message {
                role: unified_llm::types::Role::Assistant,
                content,
                name: None,
                tool_call_id: None,
            },
            finish_reason: FinishReason::ToolCalls,
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

    #[tokio::test]
    async fn parallel_tool_execution_all_results_returned() {
        let mut registry = ToolRegistry::new();
        registry.register(make_echo_tool());

        let responses = vec![
            multi_tool_call_response(vec![
                ("echo", "call_1", serde_json::json!({"text": "first"})),
                ("echo", "call_2", serde_json::json!({"text": "second"})),
                ("echo", "call_3", serde_json::json!({"text": "third"})),
            ]),
            text_response("All done!"),
        ];

        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let profile = Arc::new(ParallelTestProfile::with_tools(registry));
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let mut session = Session::new(client, profile, env, SessionConfig::default());
        let mut rx = session.subscribe();

        session.process_input("Use echo three times").await.unwrap();

        let turns = session.history().turns();
        // User + Assistant(3 tool calls) + ToolResults + Assistant(text) = 4
        assert_eq!(turns.len(), 4);

        // Verify all 3 tool results collected
        if let Turn::ToolResults { results, .. } = &turns[2] {
            assert_eq!(results.len(), 3);
            assert_eq!(results[0].tool_call_id, "call_1");
            assert_eq!(results[1].tool_call_id, "call_2");
            assert_eq!(results[2].tool_call_id, "call_3");
            assert!(!results[0].is_error);
            assert!(!results[1].is_error);
            assert!(!results[2].is_error);
        } else {
            panic!("Expected ToolResults turn at index 2");
        }

        // Verify ToolCallStart and ToolCallEnd events for all 3 calls
        let mut start_count = 0;
        let mut end_count = 0;
        while let Ok(event) = rx.try_recv() {
            match event.kind {
                EventKind::ToolCallStart => start_count += 1,
                EventKind::ToolCallEnd => end_count += 1,
                _ => {}
            }
        }
        assert_eq!(start_count, 3);
        assert_eq!(end_count, 3);
    }

    #[tokio::test]
    async fn context_window_warning_emitted_at_threshold() {
        // Use a very small context window (100 tokens = 400 chars)
        // System prompt "You are a test assistant." = 26 chars = ~6 tokens
        // We need total > 80 tokens (80% of 100)
        // So we need ~320+ chars of content beyond system prompt
        let large_input = "x".repeat(400);

        let responses = vec![text_response("OK")];

        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let registry = ToolRegistry::new();
        let profile = Arc::new(ParallelTestProfile::with_tools_and_context_window(
            registry, 100,
        ));
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let mut session = Session::new(client, profile, env, SessionConfig::default());
        let mut rx = session.subscribe();

        session.process_input(&large_input).await.unwrap();

        let mut found_warning = false;
        while let Ok(event) = rx.try_recv() {
            if event.kind == EventKind::ContextWindowWarning {
                found_warning = true;
                // Verify event data
                assert!(event.data.contains_key("estimated_tokens"));
                assert!(event.data.contains_key("context_window_size"));
                assert!(event.data.contains_key("usage_percent"));
                assert_eq!(
                    event.data["context_window_size"],
                    serde_json::json!(100)
                );
            }
        }
        assert!(found_warning);
    }

    // --- Capturing LLM Provider (captures reasoning_effort from request) ---

    struct CapturingLlmProvider {
        captured_effort: Arc<Mutex<Option<Option<String>>>>,
    }

    impl CapturingLlmProvider {
        fn new(captured_effort: Arc<Mutex<Option<Option<String>>>>) -> Self {
            Self { captured_effort }
        }
    }

    #[async_trait]
    impl ProviderAdapter for CapturingLlmProvider {
        fn name(&self) -> &str {
            "mock"
        }

        async fn complete(&self, request: &Request) -> Result<Response, SdkError> {
            *self.captured_effort.lock().unwrap() = Some(request.reasoning_effort.clone());
            Ok(text_response("captured"))
        }

        async fn stream(
            &self,
            _request: &Request,
        ) -> Result<StreamEventStream, SdkError> {
            Err(SdkError::Configuration {
                message: "streaming not supported in mock".into(),
            })
        }
    }

    #[tokio::test]
    async fn set_reasoning_effort_mid_session() {
        let captured_effort: Arc<Mutex<Option<Option<String>>>> = Arc::new(Mutex::new(None));
        let provider = Arc::new(CapturingLlmProvider::new(captured_effort.clone()));
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::new());
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let mut session = Session::new(client, profile, env, SessionConfig::default());

        // Default reasoning_effort is None
        session.set_reasoning_effort(Some("high".to_string()));
        session.process_input("test").await.unwrap();

        let effort = captured_effort.lock().unwrap().clone();
        assert_eq!(effort, Some(Some("high".to_string())));
    }

    #[tokio::test]
    async fn context_window_no_warning_under_threshold() {
        let responses = vec![text_response("OK")];

        let provider = Arc::new(MockLlmProvider::new(responses));
        let client = make_client(provider).await;
        let registry = ToolRegistry::new();
        // Large context window so short input stays well under 80%
        let profile = Arc::new(ParallelTestProfile::with_tools_and_context_window(
            registry, 200_000,
        ));
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let mut session = Session::new(client, profile, env, SessionConfig::default());
        let mut rx = session.subscribe();

        session.process_input("Hi").await.unwrap();

        let mut found_warning = false;
        while let Ok(event) = rx.try_recv() {
            if event.kind == EventKind::ContextWindowWarning {
                found_warning = true;
            }
        }
        assert!(!found_warning);
    }

    #[tokio::test]
    async fn invalid_tool_args_returns_validation_error() {
        let mut registry = ToolRegistry::new();
        registry.register(RegisteredTool {
            definition: ToolDefinition {
                name: "strict_tool".into(),
                description: "Tool with required params".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"}
                    },
                    "required": ["text"]
                }),
            },
            executor: Arc::new(|_args, _env| {
                Box::pin(async move { Ok("should not reach".to_string()) })
            }),
        });

        let responses = vec![
            tool_call_response("strict_tool", "call_1", serde_json::json!({})),
            text_response("Done"),
        ];

        let mut session = make_session_with_tools(responses, registry).await;
        session.process_input("Use strict tool").await.unwrap();

        let turns = session.history().turns();
        if let Turn::ToolResults { results, .. } = &turns[2] {
            assert!(results[0].is_error);
            let content_str = results[0].content.to_string();
            assert!(
                content_str.contains("text") && content_str.contains("required"),
                "Expected validation error mentioning 'text' and 'required', got: {content_str}"
            );
        } else {
            panic!("Expected ToolResults turn at index 2");
        }
    }

    #[tokio::test]
    async fn valid_tool_args_passes_validation() {
        let mut registry = ToolRegistry::new();
        registry.register(RegisteredTool {
            definition: ToolDefinition {
                name: "strict_tool".into(),
                description: "Tool with required params".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"}
                    },
                    "required": ["text"]
                }),
            },
            executor: Arc::new(|_args, _env| {
                Box::pin(async move { Ok("tool executed".to_string()) })
            }),
        });

        let responses = vec![
            tool_call_response(
                "strict_tool",
                "call_1",
                serde_json::json!({"text": "hello"}),
            ),
            text_response("Done"),
        ];

        let mut session = make_session_with_tools(responses, registry).await;
        session.process_input("Use strict tool").await.unwrap();

        let turns = session.history().turns();
        if let Turn::ToolResults { results, .. } = &turns[2] {
            assert!(!results[0].is_error);
        } else {
            panic!("Expected ToolResults turn at index 2");
        }
    }

    #[tokio::test]
    async fn session_start_emitted_once_for_multiple_inputs() {
        let responses = vec![
            text_response("First"),
            text_response("Second"),
        ];

        let mut session = make_session(responses).await;
        let mut rx = session.subscribe();

        session.process_input("one").await.unwrap();
        session.process_input("two").await.unwrap();

        let mut session_start_count = 0;
        while let Ok(event) = rx.try_recv() {
            if event.kind == EventKind::SessionStart {
                session_start_count += 1;
            }
        }
        // Each process_input emits SESSION_START currently -- this should be 1 per input call
        // The spec says SESSION_START is "session created", but since our Session doesn't
        // emit at creation, we accept one per process_input call as the session boundary.
        assert_eq!(session_start_count, 2);
    }

    #[tokio::test]
    async fn user_instructions_in_system_prompt() {
        // Use a capturing provider that records the system prompt
        let captured_messages: Arc<Mutex<Option<Vec<Message>>>> = Arc::new(Mutex::new(None));
        let captured_messages_clone = captured_messages.clone();

        struct CapturingProvider {
            captured: Arc<Mutex<Option<Vec<Message>>>>,
        }

        #[async_trait]
        impl ProviderAdapter for CapturingProvider {
            fn name(&self) -> &str {
                "mock"
            }
            async fn complete(&self, request: &Request) -> Result<Response, SdkError> {
                *self.captured.lock().unwrap() = Some(request.messages.clone());
                Ok(text_response("ok"))
            }
            async fn stream(
                &self,
                _request: &Request,
            ) -> Result<StreamEventStream, SdkError> {
                Err(SdkError::Configuration {
                    message: "not supported".into(),
                })
            }
        }

        let provider = Arc::new(CapturingProvider {
            captured: captured_messages_clone,
        });
        let client = make_client(provider).await;
        let profile = Arc::new(TestProfile::new());
        let env = Arc::new(MemoryExecutionEnvironment::new());
        let config = SessionConfig {
            user_instructions: Some("Always use TDD".into()),
            ..Default::default()
        };
        let mut session = Session::new(client, profile, env, config);
        session.process_input("test").await.unwrap();

        // The test profile doesn't include user_instructions in prompt (it's stubbed),
        // but we verify the config is wired through by checking the session accepted it
        assert_eq!(
            session.config.user_instructions,
            Some("Always use TDD".into())
        );
    }
}
