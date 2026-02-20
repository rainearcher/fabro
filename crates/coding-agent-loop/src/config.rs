use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct SessionConfig {
    pub max_turns: usize,
    pub max_tool_rounds_per_input: usize,
    pub default_command_timeout_ms: u64,
    pub max_command_timeout_ms: u64,
    pub reasoning_effort: Option<String>,
    pub tool_output_limits: HashMap<String, usize>,
    pub tool_line_limits: HashMap<String, usize>,
    pub enable_loop_detection: bool,
    pub loop_detection_window: usize,
    pub max_subagent_depth: usize,
    pub git_root: Option<String>,
    pub user_instructions: Option<String>,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_turns: 0,
            max_tool_rounds_per_input: 200,
            default_command_timeout_ms: 10_000,
            max_command_timeout_ms: 600_000,
            reasoning_effort: None,
            tool_output_limits: HashMap::new(),
            tool_line_limits: HashMap::new(),
            enable_loop_detection: true,
            loop_detection_window: 10,
            max_subagent_depth: 1,
            git_root: None,
            user_instructions: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_values() {
        let config = SessionConfig::default();
        assert_eq!(config.max_turns, 0);
        assert_eq!(config.max_tool_rounds_per_input, 200);
        assert_eq!(config.default_command_timeout_ms, 10_000);
        assert_eq!(config.max_command_timeout_ms, 600_000);
        assert!(config.reasoning_effort.is_none());
        assert!(config.tool_output_limits.is_empty());
        assert!(config.tool_line_limits.is_empty());
        assert!(config.enable_loop_detection);
        assert_eq!(config.loop_detection_window, 10);
        assert_eq!(config.max_subagent_depth, 1);
        assert!(config.user_instructions.is_none());
    }

    #[test]
    fn config_with_custom_values() {
        let config = SessionConfig {
            max_turns: 50,
            reasoning_effort: Some("high".into()),
            ..Default::default()
        };
        assert_eq!(config.max_turns, 50);
        assert_eq!(config.reasoning_effort, Some("high".into()));
        assert_eq!(config.max_tool_rounds_per_input, 200);
    }
}
