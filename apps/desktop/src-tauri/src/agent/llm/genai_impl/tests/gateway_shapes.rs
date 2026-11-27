//! What reaches the seam when the endpoint isn't a well-behaved OpenAI-compatible API, through
//! the REAL genai OpenAI adapter against a local server that answers the chat POST (and the
//! model list the silent-stream check asks) with a gateway's shape.
//!
//! Regression anchor `ERR-V6AR3`: a custom endpoint set to a GitHub Enterprise web root ended
//! every turn as "The reply didn't finish", with nothing in the log to say why. genai reads a 2xx
//! answer as server-sent events whatever it is, so a page parses as zero events.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use super::super::GenaiAgentLlm;
use crate::agent::llm::AgentLlm;
use crate::agent::llm::types::{AgentDelta, AgentLlmError, AgentMessage, AgentPart, AgentRole};
use crate::ai::client::AiBackend;

/// A recorded answer: status, content type, body.
#[derive(Clone, Copy)]
struct Shape {
    status: StatusCode,
    content_type: &'static str,
    body: &'static str,
}

impl IntoResponse for Shape {
    fn into_response(self) -> Response {
        (self.status, [(header::CONTENT_TYPE, self.content_type)], self.body).into_response()
    }
}

const SIGN_IN_PAGE: Shape = Shape {
    status: StatusCode::OK,
    content_type: "text/html; charset=utf-8",
    body: "<!DOCTYPE html>\n<html lang=\"en\"><head><title>Sign in</title></head><body><form></form></body></html>\n",
};

const MODEL_LIST: Shape = Shape {
    status: StatusCode::OK,
    content_type: "application/json",
    body: r#"{"object":"list","data":[{"id":"gateway-model"}]}"#,
};

/// Serves `chat` to the chat POST and `models` to `GET /v1/models`, and counts the model-list asks.
async fn gateway(chat: Shape, models: Shape) -> (AiBackend, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let counter = asked.clone();
    let app = Router::new()
        .route("/v1/chat/completions", post(move || async move { chat }))
        .route(
            "/v1/models",
            get(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                async move { models }
            }),
        );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    tokio::spawn(async move { axum::serve(listener, app).await });
    // Not a `gpt-*` id, so it goes out as plain chat completions (`remote_model_iden`).
    let backend = AiBackend::remote_for_tests(
        "k".into(),
        format!("http://127.0.0.1:{port}/v1/"),
        "gateway-model".into(),
    );
    (backend, asked)
}

/// Everything the turn would see, keep-alives dropped.
async fn run(backend: AiBackend, cancel: CancellationToken) -> Vec<Result<AgentDelta, AgentLlmError>> {
    let llm = GenaiAgentLlm::new(backend);
    let user = AgentMessage {
        role: AgentRole::User,
        parts: vec![AgentPart::Text("hi".into())],
        at: 1,
    };
    let stream = match llm
        .respond(
            "sys",
            &[],
            std::slice::from_ref(&user),
            crate::agent::llm::CallSettings::default(),
            cancel,
        )
        .await
    {
        Ok(stream) => stream,
        Err(error) => return vec![Err(error)],
    };
    stream
        .filter(|item| futures_util::future::ready(!matches!(item, Ok(AgentDelta::KeepAlive))))
        .collect()
        .await
}

fn only_error(items: Vec<Result<AgentDelta, AgentLlmError>>) -> AgentLlmError {
    match <[_; 1]>::try_from(items) {
        Ok([Err(error)]) => error,
        Ok([Ok(delta)]) => panic!("expected one error, got {delta:?}"),
        Err(items) => panic!("expected one error, got {} items: {items:?}", items.len()),
    }
}

/// The reporter's shape: the chat POST lands on a sign-in page (a 200, after a redirect), and so
/// does the model list. The turn ends saying what answered, not "didn't finish".
#[tokio::test]
async fn a_sign_in_page_at_200_ends_the_turn_as_no_ai_api() {
    let (backend, asked) = gateway(SIGN_IN_PAGE, SIGN_IN_PAGE).await;

    let error = only_error(run(backend, CancellationToken::new()).await);

    assert_eq!(error, AgentLlmError::NotAnAiApi("HTTP 200, text/html".into()));
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the silent stream asked the model list once"
    );
}

/// A website's own "Not Found" page: genai surfaces the 404 itself, and its body says no API
/// answered. No model-list ask: the stream already said why.
#[tokio::test]
async fn a_404_page_ends_the_turn_as_no_ai_api() {
    let not_found = Shape {
        status: StatusCode::NOT_FOUND,
        content_type: "text/html",
        body: "<html><body><h1>404 Not Found</h1></body></html>",
    };
    let (backend, asked) = gateway(not_found, MODEL_LIST).await;

    let error = only_error(run(backend, CancellationToken::new()).await);

    assert_eq!(error, AgentLlmError::NotAnAiApi("HTTP 404, text/html".into()));
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

/// A gateway that refuses the request with a JSON error body (here, one that won't take tools):
/// an API answered, so its own sentence reaches the rail as the detail.
#[tokio::test]
async fn a_json_error_body_carries_the_gateways_sentence() {
    let refused = Shape {
        status: StatusCode::BAD_REQUEST,
        content_type: "application/json",
        body: r#"{"error":{"message":"tools are not supported on this deployment","type":"invalid_request_error"}}"#,
    };
    let (backend, asked) = gateway(refused, MODEL_LIST).await;

    let error = only_error(run(backend, CancellationToken::new()).await);

    match error {
        AgentLlmError::Provider(detail) => assert!(
            detail.contains("tools are not supported on this deployment"),
            "the gateway's sentence rides along: {detail}"
        ),
        other => panic!("expected Provider, got {other:?}"),
    }
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}

/// A gateway that ignores `stream: true` and answers one JSON completion. genai reads no events
/// from it and keeps the body to itself, and the model list answers like an API, so nothing here
/// can say more than the turn's own "didn't finish". Pinned so a genai change that surfaces the
/// body shows up here.
#[tokio::test]
async fn a_json_body_that_isnt_a_stream_ends_with_nothing_to_add() {
    let not_streamed = Shape {
        status: StatusCode::OK,
        content_type: "application/json",
        body: r#"{"id":"c1","object":"chat.completion","choices":[{"index":0,"message":{"role":"assistant","content":"Hello"},"finish_reason":"stop"}]}"#,
    };
    let (backend, asked) = gateway(not_streamed, MODEL_LIST).await;

    let items = run(backend, CancellationToken::new()).await;

    assert!(items.is_empty(), "no End and no error: {items:?}");
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

/// A Stop ends the stream and the check with it: nothing more reaches the endpoint.
#[tokio::test]
async fn a_stopped_turn_never_asks_the_model_list() {
    let (backend, asked) = gateway(SIGN_IN_PAGE, SIGN_IN_PAGE).await;
    let cancel = CancellationToken::new();
    cancel.cancel();

    let items = run(backend, cancel).await;

    assert!(items.is_empty(), "{items:?}");
    assert_eq!(asked.load(Ordering::SeqCst), 0);
}
