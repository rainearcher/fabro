use std::sync::Mutex;

use async_trait::async_trait;

use super::{Answer, Interviewer, Question};

/// Wraps another interviewer and records all question-answer pairs.
pub struct RecordingInterviewer {
    inner: Box<dyn Interviewer>,
    recordings: Mutex<Vec<(Question, Answer)>>,
}

impl RecordingInterviewer {
    #[must_use] 
    pub fn new(inner: Box<dyn Interviewer>) -> Self {
        Self {
            inner,
            recordings: Mutex::new(Vec::new()),
        }
    }

    /// # Panics
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn recordings(&self) -> Vec<(Question, Answer)> {
        self.recordings.lock().expect("recordings lock poisoned").clone()
    }
}

#[async_trait]
impl Interviewer for RecordingInterviewer {
    async fn ask(&self, question: Question) -> Answer {
        let answer = self.inner.ask(question.clone()).await;
        self.recordings
            .lock()
            .expect("recordings lock poisoned")
            .push((question, answer.clone()));
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interviewer::auto_approve::AutoApproveInterviewer;
    use crate::interviewer::{AnswerValue, QuestionType};

    #[tokio::test]
    async fn records_question_answer_pairs() {
        let inner = Box::new(AutoApproveInterviewer);
        let recorder = RecordingInterviewer::new(inner);

        let q1 = Question::new("approve?", QuestionType::YesNo);
        let q2 = Question::new("confirm?", QuestionType::Confirmation);

        let a1 = recorder.ask(q1).await;
        assert_eq!(a1.value, AnswerValue::Yes);

        let a2 = recorder.ask(q2).await;
        assert_eq!(a2.value, AnswerValue::Yes);

        let recs = recorder.recordings();
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].0.text, "approve?");
        assert_eq!(recs[1].0.text, "confirm?");
    }

    #[tokio::test]
    async fn delegates_to_inner() {
        let inner = Box::new(AutoApproveInterviewer);
        let recorder = RecordingInterviewer::new(inner);

        let q = Question::new("text input", QuestionType::Freeform);
        let answer = recorder.ask(q).await;
        assert_eq!(answer.value, AnswerValue::Text("auto-approved".to_string()));
    }

    #[tokio::test]
    async fn recordings_empty_initially() {
        let inner = Box::new(AutoApproveInterviewer);
        let recorder = RecordingInterviewer::new(inner);
        assert!(recorder.recordings().is_empty());
    }
}
