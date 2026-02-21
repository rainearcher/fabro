use std::path::Path;

use async_trait::async_trait;

use crate::context::Context;
use crate::error::AttractorError;
use crate::graph::{Graph, Node};
use crate::outcome::Outcome;

use super::Handler;

/// No-op handler for pipeline entry point. Returns SUCCESS immediately.
pub struct StartHandler;

#[async_trait]
impl Handler for StartHandler {
    async fn execute(
        &self,
        _node: &Node,
        _context: &Context,
        _graph: &Graph,
        _logs_root: &Path,
    ) -> Result<Outcome, AttractorError> {
        Ok(Outcome::success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn start_handler_returns_success() {
        let handler = StartHandler;
        let node = Node::new("start");
        let context = Context::new();
        let graph = Graph::new("test");
        let logs_root = Path::new("/tmp/test");
        let outcome = handler
            .execute(&node, &context, &graph, logs_root)
            .await
            .unwrap();
        assert_eq!(outcome.status, crate::outcome::StageStatus::Success);
    }
}
