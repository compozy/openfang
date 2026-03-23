//! Contract tests for the Codex provider using a fixture app-server.

use std::{path::PathBuf, time::Duration};

use arky_codex::{ApprovalMode, CodexProcessConfig, CodexProvider, CodexProviderConfig};
use arky_protocol::{Message, ModelRef, SessionRef, TurnContext, TurnId};
use arky_provider::{Provider, ProviderContractCase, ProviderContractTests, ProviderRequest};
use tempfile::TempDir;

#[tokio::test]
async fn codex_provider_should_pass_contract_tests_against_fixture_server() {
    let tempdir = TempDir::new().expect("tempdir should create");
    let provider = fixture_provider(&tempdir);
    let request = ProviderRequest::new(
        SessionRef::new(None),
        TurnContext::new(TurnId::new(), 1),
        ModelRef::new("gpt-5"),
        vec![Message::user("hello")],
    );
    let case = ProviderContractCase {
        request,
        expected_descriptor: provider.descriptor().clone(),
        expected_message: Message::assistant("turn=1;echo=User: hello"),
        min_event_count: 5,
    };

    ProviderContractTests::assert_provider(&provider, &case)
        .await
        .expect("fixture-backed provider should satisfy the shared contract");
}

fn fixture_provider(tempdir: &TempDir) -> CodexProvider {
    let mut config = CodexProviderConfig {
        binary: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/fake_codex_app_server.js")
            .display()
            .to_string(),
        process: CodexProcessConfig {
            allow_npx: false,
            ..CodexProcessConfig::default()
        },
        request_timeout: Duration::from_secs(5),
        scheduler_timeout: Duration::from_secs(5),
        approval_mode: ApprovalMode::AutoApprove,
        ..CodexProviderConfig::default()
    };
    config
        .env
        .insert("ARKY_CODEX_FIXTURE".to_owned(), "1".to_owned());
    config.env.insert(
        "ARKY_CODEX_FIXTURE_STATE".to_owned(),
        tempdir
            .path()
            .join("fixture-state.json")
            .display()
            .to_string(),
    );

    CodexProvider::with_config(config)
}
