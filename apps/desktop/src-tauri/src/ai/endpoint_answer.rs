//! Whether an HTTP answer came from an AI API at all, or from something else living at that
//! address: a website, the sign-in page a redirect landed on, a proxy's notice. The settings
//! check (`connection_check`) and the chat path (`client::classify`, the agent's silent-stream
//! check) read it here, so all of them call the same answer the same thing.
//!
//! Decided by the content type and the body's SHAPE (does it parse as JSON, does it start with
//! markup), never by its wording.

/// What answered instead of an AI API. The UI words each in its own translated copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum NonApiAnswer {
    /// An HTML page, by its content type or its first character.
    WebPage,
    /// Neither JSON nor markup: plain text, or an empty body.
    NotJson,
    /// JSON, but a 404 or 405: an API lives here, just not this one, or the path is off (a base
    /// URL missing its `/v1`).
    NothingHere,
    /// JSON at 200, but not a model list: some other service's API.
    NotAModelList,
}

/// `Some` when `body` isn't JSON: [`NonApiAnswer::WebPage`] for markup, else
/// [`NonApiAnswer::NotJson`]. `None` when it parses as JSON, whatever its shape.
pub(crate) fn non_json_answer(content_type: &str, body: &str) -> Option<NonApiAnswer> {
    let body = body.trim_start();
    if serde_json::from_str::<serde::de::IgnoredAny>(body).is_ok() {
        return None;
    }
    let markup = media_type(content_type).contains("html") || body.starts_with('<');
    Some(if markup {
        NonApiAnswer::WebPage
    } else {
        NonApiAnswer::NotJson
    })
}

/// Whether `body` is an OpenAI-shaped model list: `{ "data": [...] }`, or Together's bare
/// `[...]`. An empty list counts: the API is there, it names nothing to offer.
pub(crate) fn is_openai_model_list(body: &str) -> bool {
    match serde_json::from_str::<serde_json::Value>(body) {
        Ok(serde_json::Value::Array(_)) => true,
        Ok(value) => value.get("data").is_some_and(serde_json::Value::is_array),
        Err(_) => false,
    }
}

/// The content type without its parameters (`text/html; charset=utf-8` is `text/html`).
pub(crate) fn media_type(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or_default().trim()
}

/// The short technical line under the friendly one, `HTTP 200, text/html`: the same in every
/// language, so it rides as a detail, never as copy.
pub(crate) fn summary(status: u16, content_type: &str) -> String {
    match media_type(content_type) {
        "" => format!("HTTP {status}, no content type"),
        media => format!("HTTP {status}, {media}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_of_any_shape_is_json() {
        assert_eq!(non_json_answer("text/html", r#"{"error":"x"}"#), None);
        assert_eq!(non_json_answer("", "  [1, 2]"), None);
    }

    #[test]
    fn markup_is_a_web_page_and_the_rest_is_not_json() {
        assert_eq!(non_json_answer("text/html", "Moved"), Some(NonApiAnswer::WebPage));
        assert_eq!(non_json_answer("", "\n<!doctype html>"), Some(NonApiAnswer::WebPage));
        assert_eq!(non_json_answer("text/plain", "OK"), Some(NonApiAnswer::NotJson));
        assert_eq!(non_json_answer("application/json", ""), Some(NonApiAnswer::NotJson));
    }

    #[test]
    fn a_model_list_is_data_or_a_bare_array() {
        assert!(is_openai_model_list(r#"{"object":"list","data":[]}"#));
        assert!(is_openai_model_list(r#"[{"id":"m"}]"#));
        assert!(!is_openai_model_list(r#"{"data":{"label":"x"}}"#));
        assert!(!is_openai_model_list(r#"{"current_user_url":"x"}"#));
    }

    #[test]
    fn the_summary_drops_content_type_parameters() {
        assert_eq!(summary(200, "text/html; charset=utf-8"), "HTTP 200, text/html");
        assert_eq!(summary(404, ""), "HTTP 404, no content type");
    }
}
