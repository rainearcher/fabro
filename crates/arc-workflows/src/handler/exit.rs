use std::path::Path;

use async_trait::async_trait;

use crate::context::Context;
use crate::error::ArcError;
use crate::graph::{Graph, Node};
use crate::outcome::Outcome;

use super::{EngineServices, Handler};

/// No-op handler for pipeline exit point. Returns SUCCESS immediately.
pub struct ExitHandler;

#[async_trait]
impl Handler for ExitHandler {
    async fn execute(
        &self,
        _node: &Node,
        _context: &Context,
        _graph: &Graph,
        _logs_root: &Path,
        _services: &EngineServices,
    ) -> Result<Outcome, ArcError> {
        Ok(Outcome::success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::EventEmitter;
    use crate::handler::start::StartHandler;
    use crate::handler::HandlerRegistry;

    fn make_services() -> EngineServices {
        EngineServices {
            registry: std::sync::Arc::new(HandlerRegistry::new(Box::new(StartHandler))),
            emitter: std::sync::Arc::new(EventEmitter::new()),
            sandbox: std::sync::Arc::new(arc_agent::LocalSandbox::new(
                std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
            )),
            git_state: std::sync::RwLock::new(None),
            hook_runner: None,
        }
    }

    #[tokio::test]
    async fn exit_handler_returns_success() {
        let handler = ExitHandler;
        let node = Node::new("exit");
        let context = Context::new();
        let graph = Graph::new("test");
        let logs_root = Path::new("/tmp/test");
        let outcome = handler
            .execute(&node, &context, &graph, logs_root, &make_services())
            .await
            .unwrap();
        assert_eq!(outcome.status, crate::outcome::StageStatus::Success);
    }
}
