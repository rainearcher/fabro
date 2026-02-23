use llm::provider::ProviderAdapter;
use llm::providers::{AnthropicAdapter, GeminiAdapter, OpenAiAdapter};
use llm::types::{Message, Request};

fn make_request(model: &str) -> Request {
    Request {
        model: model.to_string(),
        messages: vec![Message::user("Say hello in exactly one word")],
        provider: None,
        tools: None,
        tool_choice: None,
        response_format: None,
        temperature: Some(0.0),
        top_p: None,
        max_tokens: Some(50),
        stop_sequences: None,
        reasoning_effort: None,
        metadata: None,
        provider_options: None,
    }
}

#[tokio::test]
#[ignore = "requires ANTHROPIC_API_KEY"]
async fn anthropic_complete() {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY must be set");
    let adapter = AnthropicAdapter::new(api_key);
    let request = make_request("claude-haiku-4-5-20251001");
    let response = adapter.complete(&request).await.unwrap();

    assert!(!response.text().is_empty(), "response text should not be empty");
    assert_eq!(response.finish_reason, llm::types::FinishReason::Stop);
    assert!(response.usage.input_tokens > 0);
    assert!(response.usage.output_tokens > 0);
    assert_eq!(response.provider, "anthropic");
}

#[tokio::test]
#[ignore = "requires OPENAI_API_KEY"]
async fn openai_complete() {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let adapter = OpenAiAdapter::new(api_key);
    let request = make_request("gpt-4o-mini");
    let response = adapter.complete(&request).await.unwrap();

    assert!(!response.text().is_empty(), "response text should not be empty");
    assert_eq!(response.finish_reason, llm::types::FinishReason::Stop);
    assert!(response.usage.input_tokens > 0);
    assert!(response.usage.output_tokens > 0);
    assert_eq!(response.provider, "openai");
}

#[tokio::test]
#[ignore = "requires GEMINI_API_KEY"]
async fn gemini_complete() {
    dotenvy::dotenv().ok();
    let api_key = std::env::var("GEMINI_API_KEY").expect("GEMINI_API_KEY must be set");
    let adapter = GeminiAdapter::new(api_key);
    let request = make_request("gemini-2.5-flash");
    let response = adapter.complete(&request).await.unwrap();

    assert!(!response.text().is_empty(), "response text should not be empty");
    assert_eq!(response.finish_reason, llm::types::FinishReason::Stop);
    assert!(response.usage.input_tokens > 0);
    assert!(response.usage.output_tokens > 0);
    assert_eq!(response.provider, "gemini");
}
