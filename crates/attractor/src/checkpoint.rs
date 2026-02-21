use std::collections::HashMap;
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::context::Context;
use crate::error::{AttractorError, Result};

/// Serializable snapshot of execution state for crash recovery and resume.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Checkpoint {
    pub timestamp: DateTime<Utc>,
    pub current_node: String,
    pub completed_nodes: Vec<String>,
    pub node_retries: HashMap<String, u32>,
    pub context_values: HashMap<String, Value>,
    pub logs: Vec<String>,
}

impl Checkpoint {
    /// Create a checkpoint from the current execution state.
    pub fn from_context(
        context: &Context,
        current_node: impl Into<String>,
        completed_nodes: Vec<String>,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            current_node: current_node.into(),
            completed_nodes,
            node_retries: HashMap::new(),
            context_values: context.snapshot(),
            logs: context.logs_snapshot(),
        }
    }

    /// Save the checkpoint as JSON to a file.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or file writing fails.
    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| AttractorError::Checkpoint(format!("serialize failed: {e}")))?;
        std::fs::write(path, json)?;
        Ok(())
    }

    /// Load a checkpoint from a JSON file.
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be read or deserialization fails.
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)?;
        let checkpoint: Self = serde_json::from_str(&data)
            .map_err(|e| AttractorError::Checkpoint(format!("deserialize failed: {e}")))?;
        Ok(checkpoint)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_context_captures_state() {
        let ctx = Context::new();
        ctx.set("key", serde_json::json!("value"));
        ctx.append_log("started");

        let cp = Checkpoint::from_context(
            &ctx,
            "node_a",
            vec!["start".to_string(), "node_a".to_string()],
        );

        assert_eq!(cp.current_node, "node_a");
        assert_eq!(cp.completed_nodes.len(), 2);
        assert_eq!(cp.completed_nodes[0], "start");
        assert_eq!(cp.completed_nodes[1], "node_a");
        assert_eq!(
            cp.context_values.get("key"),
            Some(&serde_json::json!("value"))
        );
        assert_eq!(cp.logs.len(), 1);
        assert_eq!(cp.logs[0], "started");
        assert!(cp.node_retries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("checkpoint.json");

        let ctx = Context::new();
        ctx.set("goal", serde_json::json!("test"));
        ctx.append_log("log entry");

        let mut cp = Checkpoint::from_context(&ctx, "work", vec!["start".to_string()]);
        cp.node_retries.insert("work".to_string(), 2);

        cp.save(&path).unwrap();
        let loaded = Checkpoint::load(&path).unwrap();

        assert_eq!(loaded.current_node, "work");
        assert_eq!(loaded.completed_nodes, vec!["start"]);
        assert_eq!(loaded.node_retries.get("work"), Some(&2));
        assert_eq!(
            loaded.context_values.get("goal"),
            Some(&serde_json::json!("test"))
        );
        assert_eq!(loaded.logs, vec!["log entry"]);
    }

    #[test]
    fn load_nonexistent_file() {
        let result = Checkpoint::load(Path::new("/nonexistent/checkpoint.json"));
        assert!(result.is_err());
    }

    #[test]
    fn load_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "not json").unwrap();

        let result = Checkpoint::load(&path);
        assert!(result.is_err());
    }

    #[test]
    fn serialization_roundtrip() {
        let ctx = Context::new();
        let cp = Checkpoint::from_context(&ctx, "n1", vec![]);

        let json = serde_json::to_string(&cp).unwrap();
        let deserialized: Checkpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.current_node, "n1");
    }
}
