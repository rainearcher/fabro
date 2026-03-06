//! Conformance test: every path+method in the OpenAPI spec is routable (not 405).

use std::sync::Arc;

use arc_api::jwt_auth::AuthMode;
use arc_api::server::{build_router, create_app_state};
use arc_workflows::handler::exit::ExitHandler;
use arc_workflows::handler::start::StartHandler;
use arc_workflows::handler::HandlerRegistry;
use arc_workflows::interviewer::Interviewer;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use tower::ServiceExt;

fn test_registry(_interviewer: Arc<dyn Interviewer>) -> HandlerRegistry {
    let mut registry = HandlerRegistry::new(Box::new(StartHandler));
    registry.register("start", Box::new(StartHandler));
    registry.register("exit", Box::new(ExitHandler));
    registry
}

async fn test_db() -> sqlx::SqlitePool {
    let pool = arc_db::connect_memory().await.unwrap();
    arc_db::initialize_db(&pool).await.unwrap();
    pool
}

fn load_spec() -> openapiv3::OpenAPI {
    let spec_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("docs/api-reference/arc-api.yaml");
    let text = std::fs::read_to_string(&spec_path).expect("failed to read spec");
    serde_yaml::from_str(&text).expect("failed to parse spec")
}

fn resolve_path(path: &str) -> String {
    path.replace("{id}", "test-id")
        .replace("{qid}", "test-qid")
        .replace("{stageId}", "test-stage")
        .replace("{name}", "test-name")
        .replace("{slug}", "test-slug")
}

fn methods_for_path_item(item: &openapiv3::PathItem) -> Vec<Method> {
    let mut methods = Vec::new();
    if item.get.is_some() {
        methods.push(Method::GET);
    }
    if item.post.is_some() {
        methods.push(Method::POST);
    }
    if item.put.is_some() {
        methods.push(Method::PUT);
    }
    if item.delete.is_some() {
        methods.push(Method::DELETE);
    }
    if item.patch.is_some() {
        methods.push(Method::PATCH);
    }
    methods
}

#[tokio::test]
async fn all_spec_routes_are_routable() {
    let spec = load_spec();
    let state = create_app_state(test_db().await, test_registry);
    let app = build_router(state, AuthMode::Disabled);

    let mut checked = 0;
    for (path, item) in &spec.paths.paths {
        let path_item = match item {
            openapiv3::ReferenceOr::Item(item) => item,
            openapiv3::ReferenceOr::Reference { .. } => continue,
        };

        let uri = resolve_path(path);
        for method in methods_for_path_item(path_item) {
            let mut builder = Request::builder().method(&method).uri(&uri);

            let body = if method == Method::POST {
                builder = builder.header("content-type", "application/json");
                Body::from("{}")
            } else {
                Body::empty()
            };

            let req = builder.body(body).unwrap();
            let response = app.clone().oneshot(req).await.unwrap();

            assert_ne!(
                response.status(),
                StatusCode::METHOD_NOT_ALLOWED,
                "Route {method} {path} returned 405 — not registered in the router"
            );
            checked += 1;
        }
    }

    assert!(checked > 0, "No routes were checked — is the spec empty?");
}
