use super::*;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Without cloud consent the check answers at once, typed, and reaches nothing: no key read,
/// no URL validation, no client, no request.
#[test]
fn without_cloud_consent_the_check_refuses_before_any_request() {
    let refused = refuse_without_consent(false).expect("no consent ⇒ a refusal");
    assert!(refused.cloud_consent_missing);
    assert!(refused.models.is_empty());
    assert_eq!(refused.problem, None, "a typed flag, not a problem");
}

/// The organization's policy refuses before consent, typed, and reaches nothing.
#[test]
fn a_refused_endpoint_is_answered_without_a_request() {
    use crate::managed_policy::testing::{self, ALLOWED_CLOUD_AI_HOSTS, DISABLE_CLOUD_AI};
    let cloud_off = testing::forcing(&[DISABLE_CLOUD_AI]);
    let refused = refuse_by_policy(&cloud_off, "http://localhost:11434/v1").expect("cloud off ⇒ a refusal");
    assert_eq!(refused.managed, Some(ManagedAiRefusal::CloudAiOff));
    assert!(!refused.cloud_consent_missing && refused.problem.is_none());

    let listed = testing::from_values(&[(
        ALLOWED_CLOUD_AI_HOSTS,
        plist::Value::Array(vec![plist::Value::String("api.openai.com".into())]),
    )]);
    assert_eq!(
        refuse_by_policy(&listed, "https://evil.example.com/v1").and_then(|r| r.managed),
        Some(ManagedAiRefusal::HostNotAllowed)
    );
    assert!(refuse_by_policy(&listed, "https://api.openai.com/v1").is_none());
    assert!(refuse_by_policy(&ManagedPolicy::default(), "https://evil.example.com/v1").is_none());
}

/// An allowed endpoint answering 302 must not carry the probe to a host the policy refuses:
/// the probe follows redirects through the same guard as every LLM request.
#[tokio::test]
async fn the_probe_never_follows_a_redirect_to_a_refused_host() {
    use crate::managed_policy::testing::{self, ALLOWED_CLOUD_AI_HOSTS};

    let elsewhere = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":[{"id":"m"}]}"#))
        .mount(&elsewhere)
        .await;
    let allowed = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", format!("{}/v1/models", elsewhere.uri())))
        .mount(&allowed)
        .await;

    // Both servers listen on 127.0.0.1, so the list names the allowed one by its port.
    let entry = format!("127.0.0.1:{}", allowed.address().port());
    let _policy = testing::override_for_test(testing::from_values(&[(
        ALLOWED_CLOUD_AI_HOSTS,
        plist::Value::Array(vec![plist::Value::String(entry)]),
    )]));
    let result = probe_ai_endpoint(
        format!("{}/v1", allowed.uri()),
        String::new(),
        ModelListProtocol::OpenAiCompatible,
    )
    .await;

    assert!(result.models.is_empty(), "the probe stopped at the 302");
    let reached = elsewhere.received_requests().await.map_or(0, |r| r.len());
    assert_eq!(reached, 0, "the redirect target saw nothing");
}

#[test]
fn with_cloud_consent_the_check_goes_ahead() {
    assert!(refuse_without_consent(true).is_none());
}

#[test]
fn scrub_secrets_removes_bearer_tokens() {
    let out = scrub_secrets("error: invalid auth Bearer sk-abc123secret rejected", "");
    assert!(out.contains("Bearer <redacted>"), "got: {out}");
    assert!(!out.contains("sk-abc123secret"), "token leaked: {out}");
}

#[test]
fn scrub_secrets_removes_the_key_wherever_it_appears() {
    let out = scrub_secrets(r#"{"echo":{"x-api-key":"sk-ant-secret"}}"#, "sk-ant-secret");
    assert!(!out.contains("sk-ant-secret"), "key leaked: {out}");
    assert!(out.contains("<redacted>"), "got: {out}");
}

/// A mock server answering `GET <at>` with `response`, for each `(at, response)` pair.
async fn server_with(routes: Vec<(&str, ResponseTemplate)>) -> MockServer {
    let server = MockServer::start().await;
    for (at, response) in routes {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(response)
            .mount(&server)
            .await;
    }
    server
}

fn ok_json(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_string(body.to_string())
}

async fn requests(server: &MockServer) -> Vec<wiremock::Request> {
    server.received_requests().await.expect("request recording is on")
}

fn header(request: &wiremock::Request, name: &str) -> Option<String> {
    request
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// Regression anchor: the probe sent Anthropic only `Authorization: Bearer`, which it answers
/// with 400 "anthropic-version: header is required", so every Anthropic user saw an error on
/// a working key and never got a model list.
#[tokio::test]
async fn the_anthropic_probe_sends_its_native_headers_and_keeps_newest_first() {
    let body = r#"{"data":[{"id":"claude-opus-5-5","type":"model"},{"id":"claude-haiku-4-5","type":"model"}],"has_more":false}"#;
    let server = server_with(vec![("/v1/models", ok_json(body))]).await;

    let result = probe_ai_endpoint(
        format!("{}/v1/", server.uri()),
        "sk-ant-test".into(),
        ModelListProtocol::Anthropic,
    )
    .await;

    assert_eq!(result.problem, None);
    assert_eq!(
        result.models,
        ["claude-opus-5-5", "claude-haiku-4-5"],
        "Anthropic's own order"
    );
    let requests = requests(&server).await;
    let [request] = requests.as_slice() else {
        panic!("exactly one probe, got {}", requests.len())
    };
    assert_eq!(header(request, "x-api-key").as_deref(), Some("sk-ant-test"));
    assert_eq!(
        header(request, "anthropic-version").as_deref(),
        Some(ANTHROPIC_API_VERSION)
    );
    assert_eq!(
        header(request, "authorization"),
        None,
        "the key travels once, in its native header"
    );
    assert_eq!(
        request.url.query(),
        Some("limit=1000"),
        "one page holds the whole catalog"
    );
}

#[tokio::test]
async fn an_openai_compatible_probe_sends_a_bearer_key_and_sorts_the_list() {
    let server = server_with(vec![(
        "/v1/models",
        ok_json(r#"{"data":[{"id":"gpt-4.1-mini"},{"id":"dall-e-3"},{"id":"gpt-4.1-mini"}]}"#),
    )])
    .await;

    let result = probe_ai_endpoint(
        format!("{}/v1", server.uri()),
        "sk-test".into(),
        ModelListProtocol::OpenAiCompatible,
    )
    .await;

    assert_eq!(result.models, ["dall-e-3", "gpt-4.1-mini"], "sorted and deduplicated");
    let requests = requests(&server).await;
    assert_eq!(header(&requests[0], "authorization").as_deref(), Some("Bearer sk-test"));
    assert_eq!(header(&requests[0], "x-api-key"), None);
}

/// A model the provider itself marks as unable to chat never reaches the picker, where choosing
/// it would fail every request: Mistral's `capabilities.completion_chat: false` (embeddings, OCR,
/// moderation), Groq's speech and transcription models (`output_modalities` without `text`), and
/// Fireworks' `kind: EMBEDDING_MODEL` (rerankers too, which still claim `supports_chat`). Shapes
/// from each live `/models` (2026-10-27). An entry with no such marker stays: OpenAI marks none.
#[test]
fn models_the_provider_marks_as_unable_to_chat_are_not_offered() {
    let body = r#"{"data":[
        {"id":"magistral-medium-latest","capabilities":{"completion_chat":true,"reasoning":true}},
        {"id":"mistral-embed","capabilities":{"completion_chat":false}},
        {"id":"openai/gpt-oss-120b","input_modalities":["text"],"output_modalities":["text"]},
        {"id":"whisper-large-v3","input_modalities":["audio"],"output_modalities":["transcription"]},
        {"id":"canopylabs/orpheus-v1-english","input_modalities":["text"],"output_modalities":["speech"]},
        {"id":"accounts/fireworks/models/deepseek-v4p1-flash","kind":"HF_BASE_MODEL","supports_chat":true},
        {"id":"accounts/fireworks/models/qwen3-reranker-8b","kind":"EMBEDDING_MODEL","supports_chat":true},
        {"id":"gpt-4.1-mini","owned_by":"system"}
    ]}"#;
    assert_eq!(
        openai_model_ids(body),
        [
            "magistral-medium-latest",
            "openai/gpt-oss-120b",
            "accounts/fireworks/models/deepseek-v4p1-flash",
            "gpt-4.1-mini",
        ]
    );
}

/// Gemini's native list: the key in `x-goog-api-key`, only `generateContent` models, only
/// `gemini-*` ids (the ones that route natively), as bare ids.
#[tokio::test]
async fn the_gemini_probe_lists_bare_native_chat_models() {
    let body = r#"{"models":[
        {"name":"models/gemini-3.8-flash","supportedGenerationMethods":["generateContent","countTokens"]},
        {"name":"models/gemini-embedding-2","supportedGenerationMethods":["embedContent"]},
        {"name":"models/gemma-4-31b-it","supportedGenerationMethods":["generateContent"]},
        {"name":"models/gemini-3.5-flash-lite","supportedGenerationMethods":["generateContent"]}
    ]}"#;
    let server = server_with(vec![("/v1/models", ok_json(body))]).await;

    let result = probe_ai_endpoint(
        format!("{}/v1/", server.uri()),
        "AIza-test".into(),
        ModelListProtocol::Gemini,
    )
    .await;

    assert_eq!(result.models, ["gemini-3.5-flash-lite", "gemini-3.8-flash"]);
    let requests = requests(&server).await;
    assert_eq!(header(&requests[0], "x-goog-api-key").as_deref(), Some("AIza-test"));
    assert_eq!(header(&requests[0], "authorization"), None);
}

/// Google answers a bad key with 400, and says so in a typed `reason`, not the status.
#[tokio::test]
async fn a_gemini_bad_key_is_a_key_problem() {
    let body = r#"{"error":{"code":400,"message":"API key not valid. Please pass a valid API key.","status":"INVALID_ARGUMENT","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"API_KEY_INVALID","domain":"googleapis.com"}]}}"#;
    let server = server_with(vec![("/v1/models", ResponseTemplate::new(400).set_body_string(body))]).await;

    let result = probe_ai_endpoint(format!("{}/v1/", server.uri()), "bad".into(), ModelListProtocol::Gemini).await;

    assert_eq!(result.problem, Some(AiConnectionProblem::KeyRejected));
}

/// OpenRouter's `/models` is public and says 200 to any key, so a bad key would tick green.
/// The probe asks `/key` first.
#[tokio::test]
async fn the_openrouter_probe_checks_the_key_before_listing() {
    let server = server_with(vec![
        ("/api/v1/key", ResponseTemplate::new(401)),
        ("/api/v1/models", ok_json(r#"{"data":[{"id":"openai/gpt-4.1-mini"}]}"#)),
    ])
    .await;

    let result = probe_ai_endpoint(
        format!("{}/api/v1", server.uri()),
        "sk-or-bad".into(),
        ModelListProtocol::OpenRouter,
    )
    .await;

    assert_eq!(result.problem, Some(AiConnectionProblem::KeyRejected));
    assert!(result.models.is_empty());
}

#[tokio::test]
async fn the_openrouter_probe_lists_models_for_a_good_key() {
    let server = server_with(vec![
        ("/api/v1/key", ok_json(r#"{"data":{"label":"sk-or-…"}}"#)),
        (
            "/api/v1/models",
            ok_json(r#"{"data":[{"id":"z-ai/glm-5"},{"id":"openai/gpt-4.1-mini"}]}"#),
        ),
    ])
    .await;

    let result = probe_ai_endpoint(
        format!("{}/api/v1", server.uri()),
        "sk-or-good".into(),
        ModelListProtocol::OpenRouter,
    )
    .await;

    assert_eq!(result.problem, None);
    assert_eq!(result.models, ["openai/gpt-4.1-mini", "z-ai/glm-5"]);
}

/// Perplexity's base has no `/v1`, so the old `{base}/models` probe was a 404 and showed every
/// user an error. The key is checked at `/v1/models`, and nothing is offered from it.
#[tokio::test]
async fn the_perplexity_probe_checks_the_key_at_v1_and_offers_no_models() {
    let server = server_with(vec![("/v1/models", ok_json(r#"{"data":[{"id":"openai/gpt-6-sol"}]}"#))]).await;

    let result = probe_ai_endpoint(server.uri(), "pplx-test".into(), ModelListProtocol::Perplexity).await;

    assert_eq!(result.problem, None);
    assert!(
        result.models.is_empty(),
        "its agent-API models don't serve chat completions"
    );
    let requests = requests(&server).await;
    assert_eq!(
        header(&requests[0], "authorization").as_deref(),
        Some("Bearer pplx-test")
    );
}

/// A refusal carries the provider's own sentence, never its JSON envelope, and never the key.
#[tokio::test]
async fn a_refused_probe_carries_the_providers_sentence() {
    let body = r#"{"type":"error","error":{"type":"invalid_request_error","message":"Something about sk-ant-test went wrong"}}"#;
    let server = server_with(vec![("/v1/models", ResponseTemplate::new(400).set_body_string(body))]).await;

    let result = probe_ai_endpoint(
        format!("{}/v1/", server.uri()),
        "sk-ant-test".into(),
        ModelListProtocol::Anthropic,
    )
    .await;

    assert_eq!(
        result.problem,
        Some(AiConnectionProblem::ProviderRefused {
            status: 400,
            detail: "Something about <redacted> went wrong".into(),
        })
    );
}

#[tokio::test]
async fn a_rejected_key_is_a_key_problem() {
    let server = server_with(vec![("/v1/models", ResponseTemplate::new(401))]).await;
    let result = probe_ai_endpoint(
        format!("{}/v1", server.uri()),
        "sk-bad".into(),
        ModelListProtocol::OpenAiCompatible,
    )
    .await;
    assert_eq!(result.problem, Some(AiConnectionProblem::KeyRejected));
}

/// What an address that isn't an AI API answers the probe with, and what the check calls it.
async fn probe_answering(response: ResponseTemplate) -> Option<AiConnectionProblem> {
    let server = server_with(vec![("/models", response)]).await;
    probe_ai_endpoint(server.uri(), "sk-test".into(), ModelListProtocol::OpenAiCompatible)
        .await
        .problem
}

fn not_an_ai_api(answer: NonApiAnswer, detail: &str) -> Option<AiConnectionProblem> {
    Some(AiConnectionProblem::NotAnAiApi {
        answer,
        detail: detail.into(),
    })
}

/// Regression anchor (`ERR-V6AR3`): a custom endpoint set to a website's root (a GitHub
/// Enterprise host) answered `/models` with its sign-in page, a 200, and the check said
/// "Connected (model list not available)", so every chat after it ended with no reply.
#[tokio::test]
async fn a_web_page_at_200_is_not_an_ai_api() {
    let page = ResponseTemplate::new(200).set_body_raw(
        "<!DOCTYPE html><html><head><title>Sign in</title></head></html>",
        "text/html; charset=utf-8",
    );
    assert_eq!(
        probe_answering(page).await,
        not_an_ai_api(NonApiAnswer::WebPage, "HTTP 200, text/html")
    );
}

/// A page without its content type still reads as one, by its first character.
#[tokio::test]
async fn markup_without_a_content_type_is_a_web_page() {
    let page = ResponseTemplate::new(200).set_body_string("  <html><body>Hello</body></html>");
    assert_eq!(
        probe_answering(page).await.map(|p| matches!(
            p,
            AiConnectionProblem::NotAnAiApi {
                answer: NonApiAnswer::WebPage,
                ..
            }
        )),
        Some(true)
    );
}

#[tokio::test]
async fn a_web_server_404_page_is_not_an_ai_api() {
    let page = ResponseTemplate::new(404).set_body_raw("<html><body>Not Found</body></html>", "text/html");
    assert_eq!(
        probe_answering(page).await,
        not_an_ai_api(NonApiAnswer::WebPage, "HTTP 404, text/html")
    );
}

/// An API that serves something else at this path (a base URL missing its `/v1`) answers JSON,
/// but there's no model list here either.
#[tokio::test]
async fn a_json_404_means_no_ai_api_at_this_address() {
    let missing = ResponseTemplate::new(404).set_body_raw(r#"{"message":"Not Found"}"#, "application/json");
    assert_eq!(
        probe_answering(missing).await,
        not_an_ai_api(NonApiAnswer::NothingHere, "HTTP 404, application/json")
    );
}

#[tokio::test]
async fn plain_text_at_200_is_not_an_ai_api() {
    let text = ResponseTemplate::new(200).set_body_raw("OK", "text/plain");
    assert_eq!(
        probe_answering(text).await,
        not_an_ai_api(NonApiAnswer::NotJson, "HTTP 200, text/plain")
    );
}

/// JSON, but not a model list: some other service's API root.
#[tokio::test]
async fn json_that_isnt_a_model_list_is_not_an_ai_api() {
    let other = ResponseTemplate::new(200).set_body_raw(
        r#"{"current_user_url":"https://api.example.com/user"}"#,
        "application/json",
    );
    assert_eq!(
        probe_answering(other).await,
        not_an_ai_api(NonApiAnswer::NotAModelList, "HTTP 200, application/json")
    );
}

/// A real model list that happens to be empty still connects: the API is there, it just names
/// nothing to offer.
#[tokio::test]
async fn an_empty_model_list_still_connects() {
    let empty = ResponseTemplate::new(200).set_body_raw(r#"{"object":"list","data":[]}"#, "application/json");
    assert_eq!(probe_answering(empty).await, None);
}

#[tokio::test]
async fn an_insecure_endpoint_is_refused_before_any_request() {
    let result = probe_ai_endpoint(
        "http://api.example.com/v1".into(),
        "sk-secret".into(),
        ModelListProtocol::OpenAiCompatible,
    )
    .await;
    assert_eq!(result.problem, Some(AiConnectionProblem::InsecureEndpoint));
}

#[test]
fn each_special_preset_gets_its_own_protocol() {
    let cases = [
        ("anthropic", ModelListProtocol::Anthropic),
        ("google-gemini", ModelListProtocol::Gemini),
        ("openrouter", ModelListProtocol::OpenRouter),
        ("perplexity", ModelListProtocol::Perplexity),
        ("openai", ModelListProtocol::OpenAiCompatible),
        ("custom", ModelListProtocol::OpenAiCompatible),
        ("ollama", ModelListProtocol::OpenAiCompatible),
    ];
    for (id, protocol) in cases {
        assert_eq!(ModelListProtocol::for_provider(id), protocol, "{id}");
    }
}

#[test]
fn validate_url_allows_https() {
    assert!(validate_ai_base_url("https://api.openai.com/v1", "sk-secret").is_ok());
    assert!(validate_ai_base_url("https://api.openai.com/v1", "").is_ok());
}

#[test]
fn validate_url_allows_http_loopback() {
    // Ollama / LM Studio presets, with or without a key.
    assert!(validate_ai_base_url("http://localhost:11434/v1", "key").is_ok());
    assert!(validate_ai_base_url("http://127.0.0.1:1234/v1", "key").is_ok());
    assert!(validate_ai_base_url("http://[::1]:8080/v1", "key").is_ok());
    assert!(validate_ai_base_url("http://localhost:11434/v1", "").is_ok());
}

#[test]
fn validate_url_rejects_http_remote_with_key() {
    assert_eq!(
        validate_ai_base_url("http://api.openai.com/v1", "sk-secret"),
        Err(EndpointRejection::Insecure)
    );
    assert_eq!(
        validate_ai_base_url("http://10.0.0.5:1234/v1", "key"),
        Err(EndpointRejection::Insecure)
    );
}

#[test]
fn validate_url_allows_http_remote_without_key() {
    // No secret to leak, so plaintext to a remote host is allowed.
    assert!(validate_ai_base_url("http://api.openai.com/v1", "").is_ok());
    assert!(validate_ai_base_url("http://10.0.0.5:1234/v1", "").is_ok());
}

#[test]
fn validate_url_rejects_garbage() {
    for url in ["not a url", "ftp://example.com", ""] {
        assert_eq!(
            validate_ai_base_url(url, "key"),
            Err(EndpointRejection::Invalid),
            "{url}"
        );
    }
}
