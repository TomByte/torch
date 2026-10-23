use std::sync::Arc;

use futures::executor::block_on;
use torch_core::channel::ChannelState;
use torch_errors::AnyhowErrorExt as _;
use torch_server_auth::auth_state::AuthState;

use super::HttpStatusError;
use crate::auth::AuthEvent;
use crate::base_client::{AuthenticatedGraphqlConfig, BaseClient, GraphqlRoutingConfig};
struct EmptyIapTokenProvider;

impl http_client::iap::IapTokenProvider for EmptyIapTokenProvider {
    fn cached_token(&self) -> Option<String> {
        None
    }
}

fn base_client(observe_iap_challenges: bool) -> (BaseClient, async_channel::Receiver<AuthEvent>) {
    base_client_with_auth(AuthState::new_for_test(), None, observe_iap_challenges)
}

fn base_client_with_auth(
    auth_state: AuthState,
    agent_source: Option<String>,
    observe_iap_challenges: bool,
) -> (BaseClient, async_channel::Receiver<AuthEvent>) {
    let (event_sender, event_receiver) = async_channel::unbounded();
    let client = BaseClient::new(
        Arc::new(http_client::Client::new()),
        Arc::new(http_client::Client::new_external()),
        Arc::new(auth_state),
        event_sender,
        agent_source,
        GraphqlRoutingConfig::default(),
        AuthenticatedGraphqlConfig::default(),
        observe_iap_challenges.then(|| {
            Arc::new(EmptyIapTokenProvider) as Arc<dyn http_client::iap::IapTokenProvider>
        }),
    );
    client.set_ambient_workload_token_for_test("test-workload-token".to_string(), None);
    (client, event_receiver)
}

#[test]
fn iap_challenge_failure_emits_no_event_when_observation_is_disabled() {
    let _request = {
        let mut server = ChannelState::mock_server();
        server
            .mock("GET", "/api/v1/agent/identities")
            .with_status(401)
            .with_header(http_client::iap::IAP_GENERATED_RESPONSE_HEADER, "true")
            .with_body(r#"{"error":"IAP challenge"}"#)
            .create()
    };
    let (base_client, event_receiver) = base_client(false);

    block_on(base_client.get_public_api::<serde_json::Value>("agent/identities")).unwrap_err();

    assert!(event_receiver.try_recv().is_err());
}

#[test]
fn shared_status_error_actionability_ignores_retryable_client_failures() {
    let error = anyhow::Error::new(HttpStatusError::new(429, "retry later".to_string()))
        .context("Public API request failed");

    assert!(!error.is_actionable());
}
