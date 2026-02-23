use std::path::Path;

use async_trait::async_trait;

use crate::context::Context;
use crate::error::AttractorError;
use crate::graph::{Graph, Node};
use crate::outcome::Outcome;

use super::Handler;

/// Result from a `CodergenBackend` invocation.
pub enum CodergenResult {
    Text(String),
    Full(Outcome),
}

/// Backend interface for LLM execution in codergen nodes.
#[async_trait]
pub trait CodergenBackend: Send + Sync {
    async fn run(
        &self,
        node: &Node,
        prompt: &str,
        context: &Context,
        thread_id: Option<&str>,
    ) -> Result<CodergenResult, AttractorError>;
}

/// The default handler for LLM task nodes.
pub struct CodergenHandler {
    backend: Option<Box<dyn CodergenBackend>>,
}

impl CodergenHandler {
    #[must_use] 
    pub fn new(backend: Option<Box<dyn CodergenBackend>>) -> Self {
        Self { backend }
    }
}

/// Expand `$goal` in text using the graph goal.
fn expand_variables(text: &str, graph: &Graph) -> String {
    text.replace("$goal", graph.goal())
}

/// Truncate a string to at most `max_chars` characters.
fn truncate(s: &str, max_chars: usize) -> &str {
    if s.len() <= max_chars {
        s
    } else {
        &s[..max_chars]
    }
}

/// Resolve a tool hook command from node attributes, falling back to graph attributes.
fn resolve_hook(node: &Node, graph: &Graph, key: &str) -> Option<String> {
    node.attrs
        .get(key)
        .and_then(|v| v.as_str())
        .or_else(|| graph.attrs.get(key).and_then(|v| v.as_str()))
        .map(String::from)
}

/// Execute a tool hook shell command. Returns true if the command succeeded (exit 0).
fn run_hook(command: &str, node_id: &str) -> bool {
    match std::process::Command::new("sh")
        .arg("-c")
        .arg(command)
        .env("ATTRACTOR_NODE_ID", node_id)
        .output()
    {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}

#[async_trait]
impl Handler for CodergenHandler {
    async fn execute(
        &self,
        node: &Node,
        context: &Context,
        graph: &Graph,
        logs_root: &Path,
    ) -> Result<Outcome, AttractorError> {
        // 1. Build prompt
        let raw_prompt = node
            .prompt()
            .filter(|p| !p.is_empty())
            .unwrap_or_else(|| node.label());
        let prompt = expand_variables(raw_prompt, graph);

        // 2. Write prompt to logs
        let stage_dir = logs_root.join(&node.id);
        tokio::fs::create_dir_all(&stage_dir).await?;
        tokio::fs::write(stage_dir.join("prompt.md"), &prompt).await?;

        // 3. Execute pre-hook (spec 9.7)
        if let Some(pre_hook) = resolve_hook(node, graph, "tool_hooks.pre") {
            if !run_hook(&pre_hook, &node.id) {
                let mut outcome = Outcome::skipped();
                outcome.notes = Some("pre-hook returned non-zero, tool call skipped".to_string());
                return Ok(outcome);
            }
        }

        // 4. Call LLM backend
        let thread_id = context
            .get("internal.thread_id")
            .and_then(|v| v.as_str().map(String::from));
        let response_text = if let Some(backend) = &self.backend {
            match backend.run(node, &prompt, context, thread_id.as_deref()).await {
                Ok(CodergenResult::Full(outcome)) => {
                    let status_json = serde_json::to_string_pretty(&outcome)
                        .unwrap_or_else(|_| "{}".to_string());
                    tokio::fs::write(stage_dir.join("status.json"), &status_json).await?;
                    return Ok(outcome);
                }
                Ok(CodergenResult::Text(text)) => text,
                Err(e) => {
                    return Ok(Outcome::fail(e.to_string()));
                }
            }
        } else {
            format!("[Simulated] Response for stage: {}", node.id)
        };

        // 5. Execute post-hook (spec 9.7)
        if let Some(post_hook) = resolve_hook(node, graph, "tool_hooks.post") {
            if !run_hook(&post_hook, &node.id) {
                context.append_log(format!(
                    "post-hook failed for node {}, continuing",
                    node.id
                ));
            }
        }

        // 6. Write response to logs
        tokio::fs::write(stage_dir.join("response.md"), &response_text).await?;

        // 7. Build and write status
        let mut outcome = Outcome::success();
        outcome.notes = Some(format!("Stage completed: {}", node.id));
        outcome.context_updates.insert(
            "last_stage".to_string(),
            serde_json::json!(node.id),
        );
        outcome.context_updates.insert(
            "last_response".to_string(),
            serde_json::json!(truncate(&response_text, 200)),
        );

        let status_json = serde_json::to_string_pretty(&outcome)
            .unwrap_or_else(|_| "{}".to_string());
        tokio::fs::write(stage_dir.join("status.json"), &status_json).await?;

        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::AttrValue;
    use tempfile::TempDir;

    #[tokio::test]
    async fn codergen_handler_simulation_mode() {
        let handler = CodergenHandler::new(None);
        let mut node = Node::new("plan");
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Plan the implementation".to_string()),
        );
        let context = Context::new();
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        let outcome = handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();
        assert_eq!(outcome.status, crate::outcome::StageStatus::Success);
        assert_eq!(outcome.notes.as_deref(), Some("Stage completed: plan"));

        // Check files were written
        let prompt_path = tmp.path().join("plan").join("prompt.md");
        assert!(prompt_path.exists());
        let prompt_content = std::fs::read_to_string(&prompt_path).unwrap();
        assert_eq!(prompt_content, "Plan the implementation");

        let response_path = tmp.path().join("plan").join("response.md");
        assert!(response_path.exists());
        let response_content = std::fs::read_to_string(&response_path).unwrap();
        assert!(response_content.contains("[Simulated]"));

        let status_path = tmp.path().join("plan").join("status.json");
        assert!(status_path.exists());
    }

    #[tokio::test]
    async fn codergen_handler_variable_expansion() {
        let handler = CodergenHandler::new(None);
        let mut node = Node::new("plan");
        node.attrs.insert(
            "prompt".to_string(),
            AttrValue::String("Achieve: $goal".to_string()),
        );
        let context = Context::new();
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Build a feature".to_string()),
        );
        let tmp = TempDir::new().unwrap();

        handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();

        let prompt_content =
            std::fs::read_to_string(tmp.path().join("plan").join("prompt.md")).unwrap();
        assert_eq!(prompt_content, "Achieve: Build a feature");
    }

    #[tokio::test]
    async fn codergen_handler_falls_back_to_label() {
        let handler = CodergenHandler::new(None);
        let mut node = Node::new("work");
        node.attrs.insert(
            "label".to_string(),
            AttrValue::String("Do work".to_string()),
        );
        let context = Context::new();
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();

        let prompt_content =
            std::fs::read_to_string(tmp.path().join("work").join("prompt.md")).unwrap();
        assert_eq!(prompt_content, "Do work");
    }

    #[tokio::test]
    async fn codergen_handler_context_updates() {
        let handler = CodergenHandler::new(None);
        let node = Node::new("step");
        let context = Context::new();
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        let outcome = handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();

        assert_eq!(
            outcome.context_updates.get("last_stage"),
            Some(&serde_json::json!("step"))
        );
        assert!(outcome.context_updates.contains_key("last_response"));
    }

    #[test]
    fn expand_variables_replaces_goal() {
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "goal".to_string(),
            AttrValue::String("Fix bugs".to_string()),
        );
        let result = expand_variables("Goal is: $goal, do it", &graph);
        assert_eq!(result, "Goal is: Fix bugs, do it");
    }

    #[test]
    fn truncate_short_string() {
        assert_eq!(truncate("hello", 200), "hello");
    }

    #[test]
    fn truncate_long_string() {
        let long = "a".repeat(300);
        assert_eq!(truncate(&long, 200).len(), 200);
    }

    #[tokio::test]
    async fn codergen_handler_pre_hook_failure_skips_backend() {
        let handler = CodergenHandler::new(None);
        let mut node = Node::new("step");
        node.attrs.insert(
            "tool_hooks.pre".to_string(),
            AttrValue::String("exit 1".to_string()),
        );
        let context = Context::new();
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        let outcome = handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();
        assert_eq!(outcome.status, crate::outcome::StageStatus::Skipped);
        assert!(outcome
            .notes
            .as_deref()
            .unwrap()
            .contains("pre-hook"));
    }

    #[tokio::test]
    async fn codergen_handler_pre_hook_success_continues() {
        let handler = CodergenHandler::new(None);
        let mut node = Node::new("step");
        node.attrs.insert(
            "tool_hooks.pre".to_string(),
            AttrValue::String("exit 0".to_string()),
        );
        let context = Context::new();
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        let outcome = handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();
        assert_eq!(outcome.status, crate::outcome::StageStatus::Success);
    }

    #[tokio::test]
    async fn codergen_handler_post_hook_failure_logs_warning() {
        let handler = CodergenHandler::new(None);
        let mut node = Node::new("step");
        node.attrs.insert(
            "tool_hooks.post".to_string(),
            AttrValue::String("exit 1".to_string()),
        );
        let context = Context::new();
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        let outcome = handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();
        // Post-hook failure should not fail the node
        assert_eq!(outcome.status, crate::outcome::StageStatus::Success);
    }

    #[test]
    fn resolve_hook_from_node_attr() {
        let mut node = Node::new("step");
        node.attrs.insert(
            "tool_hooks.pre".to_string(),
            AttrValue::String("echo node".to_string()),
        );
        let graph = Graph::new("test");
        assert_eq!(
            resolve_hook(&node, &graph, "tool_hooks.pre"),
            Some("echo node".to_string())
        );
    }

    #[test]
    fn resolve_hook_falls_back_to_graph() {
        let node = Node::new("step");
        let mut graph = Graph::new("test");
        graph.attrs.insert(
            "tool_hooks.pre".to_string(),
            AttrValue::String("echo graph".to_string()),
        );
        assert_eq!(
            resolve_hook(&node, &graph, "tool_hooks.pre"),
            Some("echo graph".to_string())
        );
    }

    #[test]
    fn resolve_hook_none_when_missing() {
        let node = Node::new("step");
        let graph = Graph::new("test");
        assert_eq!(resolve_hook(&node, &graph, "tool_hooks.pre"), None);
    }

    #[tokio::test]
    async fn codergen_handler_passes_thread_id_to_backend() {
        use std::sync::{Arc, Mutex};

        struct ThreadCapturingBackend {
            captured_thread_id: Arc<Mutex<Option<Option<String>>>>,
        }

        #[async_trait]
        impl CodergenBackend for ThreadCapturingBackend {
            async fn run(
                &self,
                _node: &Node,
                _prompt: &str,
                _context: &Context,
                thread_id: Option<&str>,
            ) -> Result<CodergenResult, AttractorError> {
                *self.captured_thread_id.lock().unwrap() =
                    Some(thread_id.map(String::from));
                Ok(CodergenResult::Text("ok".to_string()))
            }
        }

        let captured = Arc::new(Mutex::new(None));
        let backend = ThreadCapturingBackend {
            captured_thread_id: captured.clone(),
        };
        let handler = CodergenHandler::new(Some(Box::new(backend)));

        let node = Node::new("work");
        let context = Context::new();
        // Simulate what the engine stores in internal.thread_id
        context.set("internal.thread_id", serde_json::json!("main"));
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();

        let result = captured.lock().unwrap().clone();
        assert_eq!(result, Some(Some("main".to_string())));
    }

    #[tokio::test]
    async fn codergen_handler_passes_none_thread_id_when_absent() {
        use std::sync::{Arc, Mutex};

        struct ThreadCapturingBackend {
            captured_thread_id: Arc<Mutex<Option<Option<String>>>>,
        }

        #[async_trait]
        impl CodergenBackend for ThreadCapturingBackend {
            async fn run(
                &self,
                _node: &Node,
                _prompt: &str,
                _context: &Context,
                thread_id: Option<&str>,
            ) -> Result<CodergenResult, AttractorError> {
                *self.captured_thread_id.lock().unwrap() =
                    Some(thread_id.map(String::from));
                Ok(CodergenResult::Text("ok".to_string()))
            }
        }

        let captured = Arc::new(Mutex::new(None));
        let backend = ThreadCapturingBackend {
            captured_thread_id: captured.clone(),
        };
        let handler = CodergenHandler::new(Some(Box::new(backend)));

        let node = Node::new("work");
        let context = Context::new();
        // No thread context set
        let graph = Graph::new("test");
        let tmp = TempDir::new().unwrap();

        handler
            .execute(&node, &context, &graph, tmp.path())
            .await
            .unwrap();

        let result = captured.lock().unwrap().clone();
        assert_eq!(result, Some(None));
    }
}
