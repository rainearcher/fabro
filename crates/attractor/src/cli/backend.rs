use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use agent::{
    AnthropicProfile, GeminiProfile, LocalExecutionEnvironment, OpenAiProfile, ProviderProfile,
    Session, SessionConfig, Turn,
};
use llm::client::Client;

use crate::context::Context;
use crate::error::AttractorError;
use crate::graph::Node;
use crate::handler::codergen::{CodergenBackend, CodergenResult};

/// LLM backend that delegates to an `agent` Session per invocation.
pub struct AgentBackend {
    model: String,
    provider: Option<String>,
}

impl AgentBackend {
    #[must_use]
    pub const fn new(model: String, provider: Option<String>) -> Self {
        Self { model, provider }
    }

    fn build_profile(&self) -> Arc<dyn ProviderProfile> {
        let provider = self.provider.as_deref().unwrap_or("anthropic");
        match provider {
            "openai" => Arc::new(OpenAiProfile::new(&self.model)),
            "gemini" => Arc::new(GeminiProfile::new(&self.model)),
            _ => Arc::new(AnthropicProfile::new(&self.model)),
        }
    }
}

#[async_trait]
impl CodergenBackend for AgentBackend {
    async fn run(
        &self,
        node: &Node,
        prompt: &str,
        _context: &Context,
        _thread_id: Option<&str>,
    ) -> Result<CodergenResult, AttractorError> {
        let client = Client::from_env()
            .await
            .map_err(|e| AttractorError::Handler(format!("Failed to create LLM client: {e}")))?;

        let profile = self.build_profile();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let exec_env = Arc::new(LocalExecutionEnvironment::new(cwd));

        let config = SessionConfig {
            reasoning_effort: Some(node.reasoning_effort().to_string()),
            ..SessionConfig::default()
        };

        let mut session = Session::new(client, profile, exec_env, config);
        session.initialize().await;
        session.process_input(prompt).await.map_err(|e| {
            AttractorError::Handler(format!("Agent session failed: {e}"))
        })?;

        // Extract last assistant response from the session history.
        let response = session
            .history()
            .turns()
            .iter()
            .rev()
            .find_map(|turn| {
                if let Turn::Assistant { content, .. } = turn {
                    if !content.is_empty() {
                        return Some(content.clone());
                    }
                }
                None
            })
            .unwrap_or_default();

        Ok(CodergenResult::Text(response))
    }
}
