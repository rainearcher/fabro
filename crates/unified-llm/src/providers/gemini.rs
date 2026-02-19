use crate::error::{error_from_status_code, SdkError};
use crate::provider::{ProviderAdapter, StreamEventStream};
use crate::error::{ProviderErrorDetail, ProviderErrorKind};
use crate::types::{
    ContentPart, FinishReason, Message, Request, Response, Role, ToolCall, Usage,
};

/// Provider adapter for the Google Gemini `generateContent` API.
#[allow(clippy::module_name_repetitions)]
pub struct GeminiAdapter {
    api_key: String,
    client: reqwest::Client,
}

impl GeminiAdapter {
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
#[serde(rename_all = "camelCase")]
struct ApiRequest {
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<SystemInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

#[derive(serde::Serialize)]
struct Content {
    role: String,
    parts: Vec<Part>,
}

#[derive(serde::Serialize)]
struct SystemInstruction {
    parts: Vec<Part>,
}

#[derive(serde::Serialize)]
struct Part {
    text: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_p: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stop_sequences: Option<Vec<String>>,
}

// --- Response types ---

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiResponse {
    candidates: Option<Vec<Candidate>>,
    usage_metadata: Option<UsageMetadata>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Candidate {
    content: Option<CandidateContent>,
    finish_reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct CandidateContent {
    parts: Option<Vec<serde_json::Value>>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_field_names)]
struct UsageMetadata {
    prompt_token_count: Option<i64>,
    candidates_token_count: Option<i64>,
    total_token_count: Option<i64>,
}

fn map_finish_reason(reason: Option<&str>) -> FinishReason {
    match reason {
        Some("STOP") | None => FinishReason::Stop,
        Some("MAX_TOKENS") => FinishReason::Length,
        Some("SAFETY" | "RECITATION") => FinishReason::ContentFilter,
        Some(other) => FinishReason::Other(other.to_string()),
    }
}

fn parse_part(part: &serde_json::Value) -> Option<ContentPart> {
    if let Some(text) = part.get("text").and_then(serde_json::Value::as_str) {
        return Some(ContentPart::text(text));
    }
    if let Some(fc) = part.get("functionCall") {
        let name = fc.get("name")?.as_str()?.to_string();
        let args = fc
            .get("args")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
        return Some(ContentPart::tool_call(ToolCall::new(
            uuid::Uuid::new_v4().to_string(),
            name,
            args,
        )));
    }
    None
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
                .and_then(|e| e.get("status"))
                .and_then(serde_json::Value::as_str)
                .map(String::from);
            (message, error_code, Some(v))
        },
    )
}

#[allow(clippy::too_many_lines, clippy::unnecessary_literal_bound)]
#[async_trait::async_trait]
impl ProviderAdapter for GeminiAdapter {
    fn name(&self) -> &str {
        "gemini"
    }

    async fn complete(&self, request: &Request) -> Result<Response, SdkError> {
        let mut system_parts = Vec::new();
        let mut contents = Vec::new();

        for msg in &request.messages {
            if msg.role == Role::System {
                system_parts.push(Part {
                    text: msg.text(),
                });
            } else {
                let role = if msg.role == Role::Assistant {
                    "model"
                } else {
                    "user"
                };
                contents.push(Content {
                    role: role.to_string(),
                    parts: vec![Part {
                        text: msg.text(),
                    }],
                });
            }
        }

        let system_instruction = if system_parts.is_empty() {
            None
        } else {
            Some(SystemInstruction {
                parts: system_parts,
            })
        };

        let generation_config = GenerationConfig {
            temperature: request.temperature,
            max_output_tokens: request.max_tokens,
            top_p: request.top_p,
            stop_sequences: request.stop_sequences.clone(),
        };

        let api_request = ApiRequest {
            contents,
            system_instruction,
            generation_config: Some(generation_config),
        };

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            request.model, self.api_key
        );

        let http_resp = self
            .client
            .post(&url)
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
                "gemini".to_string(),
                code,
                raw,
                None,
            ));
        }

        let api_resp: ApiResponse =
            serde_json::from_str(&body).map_err(|e| SdkError::Network {
                message: format!("failed to parse Gemini response: {e}"),
            })?;

        let candidate = api_resp
            .candidates
            .as_ref()
            .and_then(|c| c.first())
            .ok_or_else(|| SdkError::Provider {
                kind: ProviderErrorKind::Server,
                detail: Box::new(ProviderErrorDetail::new("no candidates in Gemini response", "gemini")),
            })?;

        let content_parts: Vec<ContentPart> = candidate
            .content
            .as_ref()
            .and_then(|c| c.parts.as_ref())
            .map(|parts| parts.iter().filter_map(parse_part).collect())
            .unwrap_or_default();

        let finish_reason = map_finish_reason(candidate.finish_reason.as_deref());

        let usage = api_resp
            .usage_metadata
            .as_ref()
            .map_or_else(Usage::default, |u| {
                let input = u.prompt_token_count.unwrap_or(0);
                let output = u.candidates_token_count.unwrap_or(0);
                let total = u.total_token_count.unwrap_or(input + output);
                Usage {
                    input_tokens: input,
                    output_tokens: output,
                    total_tokens: total,
                    ..Usage::default()
                }
            });

        Ok(Response {
            id: uuid::Uuid::new_v4().to_string(),
            model: request.model.clone(),
            provider: "gemini".to_string(),
            message: Message {
                role: Role::Assistant,
                content: content_parts,
                name: None,
                tool_call_id: None,
            },
            finish_reason,
            usage,
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
