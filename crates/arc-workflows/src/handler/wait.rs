use std::path::Path;

use async_trait::async_trait;

use crate::context::Context;
use crate::error::ArcError;
use crate::graph::{AttrValue, Graph, Node};
use crate::outcome::Outcome;

use super::{EngineServices, Handler};

/// Sleeps for a configured duration before proceeding.
pub struct WaitHandler;

#[async_trait]
impl Handler for WaitHandler {
    async fn execute(
        &self,
        node: &Node,
        _context: &Context,
        _graph: &Graph,
        _logs_root: &Path,
        _services: &EngineServices,
    ) -> Result<Outcome, ArcError> {
        let duration = node
            .attrs
            .get("duration")
            .and_then(AttrValue::as_duration)
            .ok_or_else(|| {
                ArcError::Validation(format!(
                    "wait node {:?} is missing a valid `duration` attribute",
                    node.id
                ))
            })?;
        tokio::time::sleep(duration).await;
        Ok(Outcome::success())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::event::EventEmitter;
    use crate::handler::HandlerRegistry;

    fn make_services() -> EngineServices {
        EngineServices {
            registry: std::sync::Arc::new(HandlerRegistry::new(Box::new(
                crate::handler::start::StartHandler,
            ))),
            emitter: std::sync::Arc::new(EventEmitter::new()),
            sandbox: std::sync::Arc::new(arc_agent::LocalSandbox::new(
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )),
            git_state: std::sync::RwLock::new(None),
            hook_runner: None,
        }
    }

    #[tokio::test]
    async fn wait_timer_success_with_short_duration() {
        let handler = WaitHandler;
        let mut node = Node::new("wait60");
        node.attrs.insert(
            "duration".to_string(),
            AttrValue::Duration(Duration::from_millis(1)),
        );
        let context = Context::new();
        let graph = Graph::new("test");
        let logs_root = Path::new("/tmp/test");
        let outcome = handler
            .execute(&node, &context, &graph, logs_root, &make_services())
            .await
            .unwrap();
        assert_eq!(outcome.status, crate::outcome::StageStatus::Success);
    }

    #[tokio::test]
    async fn wait_timer_errors_without_duration() {
        let handler = WaitHandler;
        let node = Node::new("wait_no_dur");
        let context = Context::new();
        let graph = Graph::new("test");
        let logs_root = Path::new("/tmp/test");
        let result = handler
            .execute(&node, &context, &graph, logs_root, &make_services())
            .await;
        assert!(result.is_err());
    }
}
