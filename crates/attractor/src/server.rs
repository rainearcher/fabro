use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::checkpoint::Checkpoint;
use crate::context::Context;
use crate::engine::{PipelineEngine, RunConfig};
use crate::event::{EventEmitter, PipelineEvent};
use crate::handler::HandlerRegistry;
use crate::interviewer::web::WebInterviewer;
use crate::interviewer::{Answer, AnswerValue, Interviewer};

/// Status of a managed pipeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PipelineStatus {
    Running,
    Completed,
    Failed,
    Cancelled,
}

/// A pending question exposed via the API.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiQuestion {
    pub id: String,
    pub text: String,
    pub question_type: String,
}

/// Snapshot of a managed pipeline.
struct ManagedPipeline {
    status: PipelineStatus,
    error: Option<String>,
    interviewer: Arc<WebInterviewer>,
    event_tx: broadcast::Sender<PipelineEvent>,
    context: Option<Context>,
    checkpoint: Option<Checkpoint>,
    cancel_tx: Option<tokio::sync::oneshot::Sender<()>>,
    cancel_token: Arc<AtomicBool>,
}

/// Shared application state for the server.
pub struct AppState {
    pipelines: Mutex<HashMap<String, ManagedPipeline>>,
    registry_factory: Box<dyn Fn(Arc<dyn Interviewer>) -> HandlerRegistry + Send + Sync>,
}

/// Request body for POST /pipelines.
#[derive(Debug, Deserialize)]
pub struct StartPipelineRequest {
    pub dot_source: String,
}

/// Response body for POST /pipelines.
#[derive(Debug, Serialize)]
pub struct StartPipelineResponse {
    pub id: String,
}

/// Response body for GET /pipelines/{id}.
#[derive(Debug, Serialize)]
pub struct PipelineStatusResponse {
    pub id: String,
    pub status: PipelineStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Request body for POST /pipelines/{id}/questions/{qid}/answer.
#[derive(Debug, Deserialize)]
pub struct SubmitAnswerRequest {
    pub value: String,
}

/// Response for answer submission.
#[derive(Debug, Serialize)]
pub struct SubmitAnswerResponse {
    pub accepted: bool,
}

/// Build the axum Router with all pipeline endpoints.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/pipelines", post(start_pipeline))
        .route("/pipelines/{id}", get(get_pipeline_status))
        .route("/pipelines/{id}/questions", get(get_questions))
        .route(
            "/pipelines/{id}/questions/{qid}/answer",
            post(submit_answer),
        )
        .route("/pipelines/{id}/events", get(get_events))
        .route("/pipelines/{id}/checkpoint", get(get_checkpoint))
        .route("/pipelines/{id}/context", get(get_context))
        .route("/pipelines/{id}/cancel", post(cancel_pipeline))
        .with_state(state)
}

/// Create an `AppState` with the given registry factory.
///
/// The factory receives the pipeline's `WebInterviewer` so it can wire it
/// into handlers that need human-in-the-loop interaction (e.g., `WaitHumanHandler`).
pub fn create_app_state(
    registry_factory: impl Fn(Arc<dyn Interviewer>) -> HandlerRegistry + Send + Sync + 'static,
) -> Arc<AppState> {
    Arc::new(AppState {
        pipelines: Mutex::new(HashMap::new()),
        registry_factory: Box::new(registry_factory),
    })
}

async fn start_pipeline(
    State(state): State<Arc<AppState>>,
    Json(req): Json<StartPipelineRequest>,
) -> Response {
    // Parse the DOT source
    let graph = match crate::pipeline::prepare_pipeline(&req.dot_source) {
        Ok(g) => g,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()})))
                .into_response();
        }
    };

    let pipeline_id = uuid::Uuid::new_v4().to_string();
    let interviewer = Arc::new(WebInterviewer::new());
    let (event_tx, _) = broadcast::channel(256);
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel::<()>();
    let cancel_token = Arc::new(AtomicBool::new(false));

    let context = Context::new();

    // Set up event emitter that broadcasts to the channel
    let mut emitter = EventEmitter::new();
    let tx_clone = event_tx.clone();
    emitter.on_event(move |event| {
        let _ = tx_clone.send(event.clone());
    });

    let registry = (state.registry_factory)(Arc::clone(&interviewer) as Arc<dyn Interviewer>);
    let engine = PipelineEngine::with_interviewer(
        registry,
        emitter,
        Arc::clone(&interviewer) as Arc<dyn Interviewer>,
    );

    {
        let mut pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
        pipelines.insert(
            pipeline_id.clone(),
            ManagedPipeline {
                status: PipelineStatus::Running,
                error: None,
                interviewer: Arc::clone(&interviewer),
                event_tx: event_tx.clone(),
                context: Some(context.clone()),
                checkpoint: None,
                cancel_tx: Some(cancel_tx),
                cancel_token: Arc::clone(&cancel_token),
            },
        );
    }

    // Spawn pipeline execution
    let state_clone = Arc::clone(&state);
    let id_clone = pipeline_id.clone();
    tokio::spawn(async move {
        let logs_root = std::env::temp_dir().join(format!("attractor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&logs_root).expect("failed to create logs directory");
        let config = RunConfig { logs_root, cancel_token: Some(cancel_token) };

        let result = tokio::select! {
            result = engine.run(&graph, &config) => result,
            _ = cancel_rx => {
                let mut pipelines = state_clone.pipelines.lock().expect("pipelines lock poisoned");
                if let Some(pipeline) = pipelines.get_mut(&id_clone) {
                    pipeline.status = PipelineStatus::Cancelled;
                }
                return;
            }
        };

        // Save final checkpoint
        let checkpoint = Checkpoint::load(&config.logs_root.join("checkpoint.json")).ok();

        let mut pipelines = state_clone.pipelines.lock().expect("pipelines lock poisoned");
        if let Some(pipeline) = pipelines.get_mut(&id_clone) {
            match result {
                Ok(_) => {
                    pipeline.status = PipelineStatus::Completed;
                }
                Err(crate::error::AttractorError::Cancelled) => {
                    pipeline.status = PipelineStatus::Cancelled;
                }
                Err(e) => {
                    pipeline.status = PipelineStatus::Failed;
                    pipeline.error = Some(e.to_string());
                }
            }
            pipeline.checkpoint = checkpoint;
        }
    });

    (
        StatusCode::CREATED,
        Json(StartPipelineResponse { id: pipeline_id }),
    )
        .into_response()
}

async fn get_pipeline_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
    match pipelines.get(&id) {
        Some(pipeline) => (
            StatusCode::OK,
            Json(PipelineStatusResponse {
                id: id.clone(),
                status: pipeline.status.clone(),
                error: pipeline.error.clone(),
            }),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_questions(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
    match pipelines.get(&id) {
        Some(pipeline) => {
            let pending = pipeline.interviewer.pending_questions();
            let questions: Vec<ApiQuestion> = pending
                .into_iter()
                .map(|pq| ApiQuestion {
                    id: pq.id,
                    text: pq.question.text,
                    question_type: format!("{:?}", pq.question.question_type),
                })
                .collect();
            (StatusCode::OK, Json(questions)).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn submit_answer(
    State(state): State<Arc<AppState>>,
    Path((id, qid)): Path<(String, String)>,
    Json(req): Json<SubmitAnswerRequest>,
) -> Response {
    let pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
    match pipelines.get(&id) {
        Some(pipeline) => {
            let answer = Answer {
                value: AnswerValue::Text(req.value.clone()),
                selected_option: None,
                text: Some(req.value),
            };
            let accepted = pipeline.interviewer.submit_answer(&qid, answer);
            (StatusCode::OK, Json(SubmitAnswerResponse { accepted })).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_events(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let rx = {
        let pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
        match pipelines.get(&id) {
            Some(pipeline) => pipeline.event_tx.subscribe(),
            None => return StatusCode::NOT_FOUND.into_response(),
        }
    };

    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            let data = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok::<Event, std::convert::Infallible>(
                Event::default().data(data),
            ))
        }
        Err(_) => None,
    });

    Sse::new(stream).into_response()
}

async fn get_checkpoint(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
    match pipelines.get(&id) {
        Some(pipeline) => match &pipeline.checkpoint {
            Some(cp) => (StatusCode::OK, Json(cp.clone())).into_response(),
            None => (StatusCode::OK, Json(serde_json::json!(null))).into_response(),
        },
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn get_context(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
    match pipelines.get(&id) {
        Some(pipeline) => match &pipeline.context {
            Some(ctx) => (StatusCode::OK, Json(ctx.snapshot())).into_response(),
            None => (StatusCode::OK, Json(serde_json::json!({}))).into_response(),
        },
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn cancel_pipeline(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Response {
    let mut pipelines = state.pipelines.lock().expect("pipelines lock poisoned");
    match pipelines.get_mut(&id) {
        Some(pipeline) => {
            if pipeline.status != PipelineStatus::Running {
                return (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": "pipeline is not running"})),
                )
                    .into_response();
            }
            pipeline.cancel_token.store(true, Ordering::Relaxed);
            if let Some(cancel_tx) = pipeline.cancel_tx.take() {
                let _ = cancel_tx.send(());
            }
            pipeline.status = PipelineStatus::Cancelled;
            (StatusCode::OK, Json(serde_json::json!({"cancelled": true}))).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    use crate::handler::exit::ExitHandler;
    use crate::handler::start::StartHandler;

    const MINIMAL_DOT: &str = r#"digraph Test {
        graph [goal="Test"]
        start [shape=Mdiamond]
        exit  [shape=Msquare]
        start -> exit
    }"#;

    fn test_registry(_interviewer: Arc<dyn crate::interviewer::Interviewer>) -> HandlerRegistry {
        let mut registry = HandlerRegistry::new(Box::new(StartHandler));
        registry.register("start", Box::new(StartHandler));
        registry.register("exit", Box::new(ExitHandler));
        registry
    }

    fn test_app() -> Router {
        let state = create_app_state(test_registry);
        build_router(state)
    }

    async fn body_json(body: Body) -> serde_json::Value {
        let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    #[tokio::test]
    async fn post_pipelines_starts_pipeline_and_returns_id() {
        let app = test_app();

        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::CREATED);

        let body = body_json(response.into_body()).await;
        assert!(body["id"].is_string());
        assert!(!body["id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn post_pipelines_invalid_dot_returns_bad_request() {
        let app = test_app();

        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": "not a graph"})).unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn get_pipeline_status_returns_status() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Give pipeline a moment to run
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Check status
        let req = Request::builder()
            .method("GET")
            .uri(format!("/pipelines/{pipeline_id}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_json(response.into_body()).await;
        assert_eq!(body["id"].as_str().unwrap(), pipeline_id);
        // Status should be either "running" or "completed"
        let status = body["status"].as_str().unwrap();
        assert!(
            status == "running" || status == "completed",
            "unexpected status: {status}"
        );
    }

    #[tokio::test]
    async fn get_pipeline_status_not_found() {
        let app = test_app();

        let req = Request::builder()
            .method("GET")
            .uri("/pipelines/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_questions_returns_empty_list() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Get questions (should be empty for a pipeline without wait.human nodes)
        let req = Request::builder()
            .method("GET")
            .uri(format!("/pipelines/{pipeline_id}/questions"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_json(response.into_body()).await;
        assert!(body.is_array());
    }

    #[tokio::test]
    async fn submit_answer_not_found_pipeline() {
        let app = test_app();

        let req = Request::builder()
            .method("POST")
            .uri("/pipelines/nonexistent/questions/q1/answer")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"value": "yes"})).unwrap(),
            ))
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_events_not_found() {
        let app = test_app();

        let req = Request::builder()
            .method("GET")
            .uri("/pipelines/nonexistent/events")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_checkpoint_returns_null_initially() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Get checkpoint immediately (before pipeline completes, may be null)
        let req = Request::builder()
            .method("GET")
            .uri(format!("/pipelines/{pipeline_id}/checkpoint"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn get_context_returns_map() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Get context
        let req = Request::builder()
            .method("GET")
            .uri(format!("/pipelines/{pipeline_id}/context"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_json(response.into_body()).await;
        assert!(body.is_object());
    }

    #[tokio::test]
    async fn cancel_pipeline_succeeds() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Cancel it
        let req = Request::builder()
            .method("POST")
            .uri(format!("/pipelines/{pipeline_id}/cancel"))
            .body(Body::empty())
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        // Could be OK (cancelled) or CONFLICT (already completed)
        let status = response.status();
        assert!(
            status == StatusCode::OK || status == StatusCode::CONFLICT,
            "unexpected status: {status}"
        );
    }

    #[tokio::test]
    async fn cancel_nonexistent_pipeline_returns_not_found() {
        let app = test_app();

        let req = Request::builder()
            .method("POST")
            .uri("/pipelines/nonexistent/cancel")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn get_events_returns_sse_stream() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Request the SSE stream
        let req = Request::builder()
            .method("GET")
            .uri(format!("/pipelines/{pipeline_id}/events"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // Check content-type is text/event-stream
        let content_type = response
            .headers()
            .get("content-type")
            .expect("content-type header should be present")
            .to_str()
            .unwrap();
        assert!(
            content_type.contains("text/event-stream"),
            "expected text/event-stream, got: {content_type}"
        );
    }

    #[tokio::test]
    async fn pipeline_completes_and_status_is_completed() {
        let state = create_app_state(test_registry);
        let app = build_router(Arc::clone(&state));

        // Start a pipeline
        let req = Request::builder()
            .method("POST")
            .uri("/pipelines")
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::to_string(&serde_json::json!({"dot_source": MINIMAL_DOT})).unwrap(),
            ))
            .unwrap();

        let response = app.clone().oneshot(req).await.unwrap();
        let body = body_json(response.into_body()).await;
        let pipeline_id = body["id"].as_str().unwrap().to_string();

        // Wait for pipeline to complete
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        // Check status
        let req = Request::builder()
            .method("GET")
            .uri(format!("/pipelines/{pipeline_id}"))
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let body = body_json(response.into_body()).await;
        assert_eq!(body["status"].as_str().unwrap(), "completed");
    }
}
