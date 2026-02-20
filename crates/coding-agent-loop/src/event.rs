use crate::types::{EventKind, SessionEvent};
use std::collections::HashMap;
use std::time::SystemTime;
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct EventEmitter {
    sender: broadcast::Sender<SessionEvent>,
}

impl EventEmitter {
    #[must_use]
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(1024);
        Self { sender }
    }

    pub fn emit(
        &self,
        kind: EventKind,
        session_id: String,
        data: HashMap<String, serde_json::Value>,
    ) {
        let event = SessionEvent {
            kind,
            timestamp: SystemTime::now(),
            session_id,
            data,
        };
        // Ignore send error (no receivers)
        let _ = self.sender.send(event);
    }

    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<SessionEvent> {
        self.sender.subscribe()
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_and_receive_event() {
        let emitter = EventEmitter::new();
        let mut receiver = emitter.subscribe();

        emitter.emit(
            EventKind::SessionStart,
            "sess-1".into(),
            HashMap::new(),
        );

        let event = receiver.recv().await.unwrap();
        assert_eq!(event.kind, EventKind::SessionStart);
        assert_eq!(event.session_id, "sess-1");
        assert!(event.data.is_empty());
    }

    #[tokio::test]
    async fn emit_with_data() {
        let emitter = EventEmitter::new();
        let mut receiver = emitter.subscribe();

        let mut data = HashMap::new();
        data.insert("text".into(), serde_json::json!("hello world"));

        emitter.emit(EventKind::AssistantTextDelta, "sess-2".into(), data);

        let event = receiver.recv().await.unwrap();
        assert_eq!(event.kind, EventKind::AssistantTextDelta);
        assert_eq!(event.data["text"], serde_json::json!("hello world"));
    }

    #[tokio::test]
    async fn multiple_subscribers() {
        let emitter = EventEmitter::new();
        let mut rx1 = emitter.subscribe();
        let mut rx2 = emitter.subscribe();

        emitter.emit(EventKind::SessionEnd, "sess-3".into(), HashMap::new());

        let e1 = rx1.recv().await.unwrap();
        let e2 = rx2.recv().await.unwrap();
        assert_eq!(e1.kind, EventKind::SessionEnd);
        assert_eq!(e2.kind, EventKind::SessionEnd);
        assert_eq!(e1.session_id, "sess-3");
        assert_eq!(e2.session_id, "sess-3");
    }

    #[test]
    fn emit_without_subscribers_does_not_panic() {
        let emitter = EventEmitter::new();
        emitter.emit(EventKind::Error, "sess-4".into(), HashMap::new());
    }

    #[test]
    fn default_creates_emitter() {
        let emitter = EventEmitter::default();
        let _rx = emitter.subscribe();
    }
}
