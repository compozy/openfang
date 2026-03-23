//! Integration tests covering classified-error projections in protocol DTOs.

use std::{error::Error, fmt, time::Duration};

use arky_error::ClassifiedError;
use pretty_assertions::assert_eq;
use serde_json::json;

use arky_protocol::{
    AgentResponse, Message, ModelRef, ProviderRequest, SessionRef, TurnContext, TurnId,
};

#[derive(Debug)]
struct RetryableProtocolError;

impl fmt::Display for RetryableProtocolError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("protocol error for integration test")
    }
}

impl Error for RetryableProtocolError {}

impl ClassifiedError for RetryableProtocolError {
    fn error_code(&self) -> &'static str {
        "PROTOCOL_TEST_FAILURE"
    }

    fn is_retryable(&self) -> bool {
        true
    }

    fn retry_after(&self) -> Option<Duration> {
        Some(Duration::from_secs(2))
    }

    fn http_status(&self) -> u16 {
        503
    }

    fn correction_context(&self) -> Option<serde_json::Value> {
        Some(json!({ "retry": "safe" }))
    }
}

#[test]
fn protocol_responses_should_project_classified_errors() {
    let request = ProviderRequest::new(
        SessionRef::new(None),
        TurnContext::new(TurnId::new(), 1),
        ModelRef::new("claude-sonnet"),
        vec![Message::user("hello")],
    );
    let ProviderRequest { session, turn, .. } = request;
    let response = AgentResponse::new(session, turn, Message::assistant("failed"))
        .with_error(&RetryableProtocolError);

    let actual = serde_json::to_value(&response).expect("response should serialize");
    let error = actual.get("error").expect("error payload should exist");

    assert_eq!(
        error,
        &json!({
            "error_code": "PROTOCOL_TEST_FAILURE",
            "message": "protocol error for integration test",
            "is_retryable": true,
            "retry_after_ms": 2_000_u64,
            "http_status": 503_u16,
            "correction_context": { "retry": "safe" },
        })
    );
}
