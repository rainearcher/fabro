use crate::error::{error_from_status_code, SdkError};
use crate::provider::{ProviderAdapter, StreamEventStream};
use crate::types::{
    ContentPart, FinishReason, Message, Request, Response, Role, ToolCall, Usage,
};

/// Provider adapter for the Anthropic Messages API.
#[allow(clippy::module_name_repetitions)]
pub struct AnthropicAdapter {
    api_key: String,
    client: reqwest::Client,
}

impl AnthropicAdapter {
    #[must_use]
    pub fn new(api_key: impl Into<String>) -> Self {
        Self {
            api_key: api_key.into(),
            client: reqwest::Client::new(),
        }
    }
}

// --- Request types ---

#[derive(serde::Serialize)]
struct ApiMessage {
    role: String,
    content: String,
}

#[derive(serde::Serialize)]
struct ApiRequest {
    model: String,
    messages: Vec<ApiMessage>,
    max_tokens: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

// --- Response types ---

#[derive(serde::Deserialize)]
struct ApiResponse {
    id: String,
    model: String,
    content: Vec<serde_json::Value>,
    stop_reason: Option<String>,
    usage: ApiUsage,
}

#[derive(serde::Deserialize)]
struct ApiUsage {
    input_tokens: i64,
    output_tokens: i64,
}

fn map_finish_reason(stop_reason: Option<&str>) -> FinishReason {
    match stop_reason {
        Some("end_turn") | None => FinishReason::Stop,
        Some("max_tokens") => FinishReason::Length,
        Some("tool_use") => FinishReason::ToolCalls,
        Some(other) => FinishReason::Other(other.to_string()),
    }
}

fn parse_content_block(block: &serde_json::Value) -> Option<ContentPart> {
    match block.get("type")?.as_str()? {
        "text" => Some(ContentPart::text(block.get("text")?.as_str()?)),
        "tool_use" => Some(ContentPart::tool_call(ToolCall::new(
            block.get("id")?.as_str()?,
            block.get("name")?.as_str()?,
            block.get("input")?.clone(),
        ))),
        _ => None,
    }
}

fn parse_error_body(body: &str) -> (String, Option<String>, Option<serde_json::Value>) {
    serde_json::from_str::<serde_json::Value>(body).map_or_else(
        |_| (body.to_string(), None, None),
        |v| {
            let message = v
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("Unknown error")
                .to_string();
            let error_code = v
                .get("error")
                .and_then(|e| e.get("type"))
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            (message, error_code, Some(v))
        },
    )
}

#[allow(clippy::unnecessary_literal_bound)]
#[async_trait::async_trait]
impl ProviderAdapter for AnthropicAdapter {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn complete(&self, request: &Request) -> Result<Response, SdkError> {
        let mut system_parts = Vec::new();
        let mut api_messages = Vec::new();

        for msg in &request.messages {
            if msg.role == Role::System {
                system_parts.push(msg.text());
            } else {
                let role = if msg.role == Role::Assistant {
                    "assistant"
                } else {
                    "user"
                };
                api_messages.push(ApiMessage {
                    role: role.to_string(),
                    content: msg.text(),
                });
            }
        }

        let system = if system_parts.is_empty() {
            None
        } else {
            Some(system_parts.join("\n"))
        };

        let api_request = ApiRequest {
            model: request.model.clone(),
            messages: api_messages,
            max_tokens: request.max_tokens.unwrap_or(1024),
            system,
            temperature: request.temperature,
            top_p: request.top_p,
            stop_sequences: request.stop_sequences.clone(),
        };

        let http_resp = self
            .client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&api_request)
            .send()
            .await
            .map_err(|e| SdkError::Network {
                message: e.to_string(),
            })?;

        let status = http_resp.status();
        let body = http_resp
            .text()
            .await
            .map_err(|e| SdkError::Network {
                message: e.to_string(),
            })?;

        if !status.is_success() {
            let (msg, code, raw) = parse_error_body(&body);
            return Err(error_from_status_code(
                status.as_u16(),
                msg,
                "anthropic".to_string(),
                code,
                raw,
                None,
            ));
        }

        let api_resp: ApiResponse =
            serde_json::from_str(&body).map_err(|e| SdkError::Network {
                message: format!("failed to parse Anthropic response: {e}"),
            })?;

        let content_parts: Vec<ContentPart> = api_resp
            .content
            .iter()
            .filter_map(parse_content_block)
            .collect();

        let finish_reason = map_finish_reason(api_resp.stop_reason.as_deref());
        let total = api_resp.usage.input_tokens + api_resp.usage.output_tokens;

        Ok(Response {
            id: api_resp.id,
            model: api_resp.model,
            provider: "anthropic".to_string(),
            message: Message {
                role: Role::Assistant,
                content: content_parts,
                name: None,
                tool_call_id: None,
            },
            finish_reason,
            usage: Usage {
                input_tokens: api_resp.usage.input_tokens,
                output_tokens: api_resp.usage.output_tokens,
                total_tokens: total,
                ..Usage::default()
            },
            raw: serde_json::from_str(&body).ok(),
            warnings: vec![],
            rate_limit: None,
        })
    }

    async fn stream(&self, _request: &Request) -> Result<StreamEventStream, SdkError> {
        Err(SdkError::Configuration {
            message: "streaming not yet implemented".to_string(),
        })
    }
}
