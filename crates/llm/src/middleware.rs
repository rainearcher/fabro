use crate::error::SdkError;
use crate::provider::StreamEventStream;
use crate::types::{Request, Response, StreamEvent};
use futures::StreamExt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// The next handler in the middleware chain.
pub type NextFn = Arc<
    dyn Fn(Request) -> Pin<Box<dyn Future<Output = Result<Response, SdkError>> + Send>>
        + Send
        + Sync,
>;

/// The next handler for streaming.
pub type NextStreamFn = Arc<
    dyn Fn(Request) -> Pin<Box<dyn Future<Output = Result<StreamEventStream, SdkError>> + Send>>
        + Send
        + Sync,
>;

/// Middleware for intercepting `complete()` and streaming calls (Section 2.3).
///
/// Implement `handle_complete` for blocking requests and `handle_stream` for
/// streaming requests. Override `process_stream` to observe or transform
/// individual stream events without replacing the entire stream handler.
#[async_trait::async_trait]
pub trait Middleware: Send + Sync {
    async fn handle_complete(
        &self,
        request: Request,
        next: NextFn,
    ) -> Result<Response, SdkError>;

    async fn handle_stream(
        &self,
        request: Request,
        next: NextStreamFn,
    ) -> Result<StreamEventStream, SdkError>;

    /// Process an individual stream event. Override to observe or transform
    /// events as they pass through the middleware. The default implementation
    /// passes events through unchanged.
    fn process_stream_event(
        &self,
        event: Result<StreamEvent, SdkError>,
    ) -> Result<StreamEvent, SdkError> {
        event
    }
}

/// Wrap a `StreamEventStream` so that each event passes through a middleware's
/// `process_stream_event` method.
pub fn wrap_stream_with_middleware(
    stream: StreamEventStream,
    middleware: Arc<dyn Middleware>,
) -> StreamEventStream {
    Box::pin(stream.map(move |event| middleware.process_stream_event(event)))
}
