use std::collections::HashMap;
use std::time::SystemTime;
use unified_llm::types::{ToolCall, ToolResult, Usage};

#[derive(Debug, Clone)]
pub enum Turn {
    User {
        content: String,
        timestamp: SystemTime,
    },
    Assistant {
        content: String,
        tool_calls: Vec<ToolCall>,
        reasoning: Option<String>,
        usage: Usage,
        response_id: String,
        timestamp: SystemTime,
    },
    ToolResults {
        results: Vec<ToolResult>,
        timestamp: SystemTime,
    },
    System {
        content: String,
        timestamp: SystemTime,
    },
    Steering {
        content: String,
        timestamp: SystemTime,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Processing,
    AwaitingInput,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventKind {
    SessionStart,
    SessionEnd,
    UserInput,
    AssistantTextStart,
    AssistantTextDelta,
    AssistantTextEnd,
    ToolCallStart,
    ToolCallOutputDelta,
    ToolCallEnd,
    SteeringInjected,
    TurnLimit,
    LoopDetection,
    ContextWindowWarning,
    Error,
}

#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub kind: EventKind,
    pub timestamp: SystemTime,
    pub session_id: String,
    pub data: HashMap<String, serde_json::Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_user_construction() {
        let turn = Turn::User {
            content: "Hello".into(),
            timestamp: SystemTime::now(),
        };
        match &turn {
            Turn::User { content, .. } => assert_eq!(content, "Hello"),
            _ => panic!("Expected User turn"),
        }
    }

    #[test]
    fn turn_assistant_construction() {
        let turn = Turn::Assistant {
            content: "Hi there".into(),
            tool_calls: vec![],
            reasoning: None,
            usage: Usage::default(),
            response_id: "resp_1".into(),
            timestamp: SystemTime::now(),
        };
        match &turn {
            Turn::Assistant {
                content,
                tool_calls,
                reasoning,
                response_id,
                ..
            } => {
                assert_eq!(content, "Hi there");
                assert!(tool_calls.is_empty());
                assert!(reasoning.is_none());
                assert_eq!(response_id, "resp_1");
            }
            _ => panic!("Expected Assistant turn"),
        }
    }

    #[test]
    fn turn_tool_results_construction() {
        let result = ToolResult {
            tool_call_id: "call_1".into(),
            content: serde_json::json!("result"),
            is_error: false,
            image_data: None,
            image_media_type: None,
        };
        let turn = Turn::ToolResults {
            results: vec![result],
            timestamp: SystemTime::now(),
        };
        match &turn {
            Turn::ToolResults { results, .. } => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].tool_call_id, "call_1");
            }
            _ => panic!("Expected ToolResults turn"),
        }
    }

    #[test]
    fn turn_system_construction() {
        let turn = Turn::System {
            content: "System prompt".into(),
            timestamp: SystemTime::now(),
        };
        match &turn {
            Turn::System { content, .. } => assert_eq!(content, "System prompt"),
            _ => panic!("Expected System turn"),
        }
    }

    #[test]
    fn turn_steering_construction() {
        let turn = Turn::Steering {
            content: "Focus on the task".into(),
            timestamp: SystemTime::now(),
        };
        match &turn {
            Turn::Steering { content, .. } => assert_eq!(content, "Focus on the task"),
            _ => panic!("Expected Steering turn"),
        }
    }

    #[test]
    fn session_state_equality() {
        assert_eq!(SessionState::Idle, SessionState::Idle);
        assert_eq!(SessionState::Processing, SessionState::Processing);
        assert_eq!(SessionState::AwaitingInput, SessionState::AwaitingInput);
        assert_eq!(SessionState::Closed, SessionState::Closed);
        assert_ne!(SessionState::Idle, SessionState::Closed);
    }

    #[test]
    fn event_kind_equality() {
        assert_eq!(EventKind::SessionStart, EventKind::SessionStart);
        assert_ne!(EventKind::SessionStart, EventKind::SessionEnd);
        assert_eq!(EventKind::LoopDetection, EventKind::LoopDetection);
    }

    #[test]
    fn session_event_construction() {
        let event = SessionEvent {
            kind: EventKind::SessionStart,
            timestamp: SystemTime::now(),
            session_id: "sess_1".into(),
            data: HashMap::new(),
        };
        assert_eq!(event.kind, EventKind::SessionStart);
        assert_eq!(event.session_id, "sess_1");
        assert!(event.data.is_empty());
    }
}
