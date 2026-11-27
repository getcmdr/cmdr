//! What a chat stream that ended with NOTHING at all (no text, no tool call, no reasoning, no
//! `End`, no error) asks next: is there an AI API at this address at all?
//!
//! genai reads a 2xx answer as server-sent events whatever its content type, so a web page, a
//! sign-in page a redirect landed on, or a JSON body that ignored `stream: true` parses as zero
//! events and the stream just ends. genai keeps the response itself private, so the status,
//! content type, and body never reach us. This asks the endpoint's model list instead, through
//! the settings check's own classifier (`connection_check::list_models`, `endpoint_answer`), so
//! the chat names the answer the way Settings does. It's a `GET`: it never re-sends the turn, and
//! it costs no tokens.
//!
//! Regression anchor `ERR-V6AR3`: a custom endpoint set to a GitHub Enterprise web root ended
//! every turn as "The reply didn't finish", with nothing in the log to say why.

use super::AiError;
use crate::ai::connection_check::{self, AiConnectionProblem, ModelListProtocol};
use crate::ai::endpoint_answer::NonApiAnswer;
use crate::managed_policy::AiDestination;

/// A remote backend's endpoint, for the one check a silent stream earns. Holds the key because
/// the model list wants it, and goes only where the chat itself went (`destination`, judged by
/// the managed policy again before the request). Shared behind an `Arc`, so a backend stays small.
pub(crate) struct SilentStreamProbe {
    pub(super) base_url: String,
    pub(super) api_key: String,
    pub(super) destination: AiDestination,
}

impl SilentStreamProbe {
    /// `Some(NotAnAiApi)` when the endpoint's model list says no AI API lives here; `None` when
    /// it looks like one (so the reply's format is what Cmdr couldn't read) or the check itself
    /// couldn't answer, which leaves the turn's own "didn't finish".
    pub(crate) async fn diagnose(&self) -> Option<AiError> {
        if crate::managed_policy::for_egress()
            .await
            .ai_destination(&self.destination)
            .is_err()
        {
            return None;
        }
        log::info!("the chat stream ended with nothing at all; asking the endpoint's model list what answers there");
        let verdict =
            connection_check::list_models(&self.base_url, &self.api_key, ModelListProtocol::OpenAiCompatible).await;
        verdict_for(verdict.map(|_| ()))
    }
}

/// The pure half of [`SilentStreamProbe::diagnose`]. A 404 at `/models` (`NothingHere`) proves
/// nothing here: some real chat endpoints serve no model list (Perplexity's base has no `/v1`).
fn verdict_for(listed: Result<(), AiConnectionProblem>) -> Option<AiError> {
    match listed {
        Err(AiConnectionProblem::NotAnAiApi {
            answer: NonApiAnswer::WebPage | NonApiAnswer::NotJson | NonApiAnswer::NotAModelList,
            detail,
        }) => Some(AiError::NotAnAiApi(detail)),
        Ok(()) => {
            log::info!(
                "the endpoint's model list answers like an AI API, so the chat reply came in a shape Cmdr couldn't read"
            );
            None
        }
        Err(problem) => {
            log::info!("the endpoint's model list didn't settle what answers there: {problem:?}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_web_page_or_another_service_is_no_ai_api() {
        for answer in [
            NonApiAnswer::WebPage,
            NonApiAnswer::NotJson,
            NonApiAnswer::NotAModelList,
        ] {
            let verdict = verdict_for(Err(AiConnectionProblem::NotAnAiApi {
                answer,
                detail: "HTTP 200, text/html".into(),
            }));
            assert!(matches!(verdict, Some(AiError::NotAnAiApi(detail)) if detail == "HTTP 200, text/html"));
        }
    }

    /// A model list that's missing, or one that works, says nothing about the chat reply.
    #[test]
    fn a_missing_or_working_model_list_leaves_the_turn_unfinished() {
        let missing = Err(AiConnectionProblem::NotAnAiApi {
            answer: NonApiAnswer::NothingHere,
            detail: "HTTP 404, application/json".into(),
        });
        assert!(verdict_for(missing).is_none());
        assert!(verdict_for(Ok(())).is_none());
        assert!(verdict_for(Err(AiConnectionProblem::TimedOut)).is_none());
    }
}
