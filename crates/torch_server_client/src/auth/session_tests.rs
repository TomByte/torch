use std::sync::Arc;

use futures::executor::block_on;
use torch_server_auth::auth_state::AuthState;
use torch_server_auth::credentials::{Credentials, FirebaseToken, LoginToken, RefreshToken};
use torch_server_auth::user::FirebaseAuthTokens;

use super::{AuthEvent, AuthSession, UserAuthenticationError, parse_retry_after};

fn session_with_state(
    auth_state: Arc<AuthState>,
) -> (AuthSession, async_channel::Receiver<super::AuthEvent>) {
    let (event_sender, event_receiver) = async_channel::unbounded();
    let session = AuthSession::new(
        Arc::new(http_client::Client::new()),
        auth_state,
        event_sender,
    );
    (session, event_receiver)
}

fn session_with_refresh_urls(
    auth_state: Arc<AuthState>,
    direct_url: String,
    proxy_url: String,
) -> (AuthSession, async_channel::Receiver<AuthEvent>) {
    let (mut session, event_receiver) = session_with_state(auth_state);
    session.refresh_urls = Some((direct_url, proxy_url));
    (session, event_receiver)
}

fn successful_refresh_response(id_token: &str, refresh_token: &str) -> String {
    format!(r#"{{"id_token":"{id_token}","refresh_token":"{refresh_token}","expires_in":"3600"}}"#)
}
#[test]
fn retry_after_http_date_uses_remaining_delay() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-09-20T17:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);

    let delay = parse_retry_after("Sun, 20 Sep 2026 17:02:00 GMT", now);

    assert_eq!(delay, Some(instant::Duration::from_secs(120)));
}

#[test]
fn device_authorization_uses_warp_agent_cli_client() {
    let client = AuthSession::create_oauth_client();

    assert_eq!(client.client_id().as_str(), "warp-agent-cli");
}

#[test]
fn api_key_exchange_defers_owner_type_until_user_properties_are_fetched() {
    let auth_state = Arc::new(AuthState::new_logged_out_for_test());
    let (session, _) = session_with_state(auth_state);

    let credentials =
        block_on(session.exchange_credentials(LoginToken::ApiKey("api-key".to_string()))).unwrap();

    assert!(matches!(
        credentials,
        Credentials::ApiKey {
            key,
            owner_type: None
        } if key == "api-key"
    ));
}

fn fetch_access_token_response(
    status: usize,
    body: &str,
) -> Result<FirebaseAuthTokens, UserAuthenticationError> {
    let mut server = mockito::Server::new();
    let response = server
        .mock("POST", "/token")
        .with_status(status)
        .with_body(body)
        .create();
    let client = http_client::Client::new();

    let result = block_on(AuthSession::fetch_access_token(
        client.post(format!("{}/token", server.url())),
    ))
    .map_err(super::TokenRefreshError::into_user_authentication_error);

    response.assert();
    result
}

#[test]
fn rejected_refresh_token_is_classified_from_error_body() {
    let result = fetch_access_token_response(
        400,
        r#"{"error":{"code":400,"message":"INVALID_REFRESH_TOKEN"}}"#,
    );

    assert!(matches!(
        result,
        Err(UserAuthenticationError::DeniedAccessToken(error))
            if error.message == "INVALID_REFRESH_TOKEN"
    ));
}

#[test]
fn disabled_account_is_classified_from_error_body() {
    let result =
        fetch_access_token_response(400, r#"{"error":{"code":400,"message":"USER_DISABLED"}}"#);

    assert!(matches!(
        result,
        Err(UserAuthenticationError::UserAccountDisabled(error))
            if error.message == "USER_DISABLED"
    ));
}

#[test]
fn unavailable_firebase_response_is_not_a_terminal_verdict() {
    let result =
        fetch_access_token_response(503, r#"{"error":{"code":503,"message":"UNAVAILABLE"}}"#);

    assert!(matches!(
        result,
        Err(UserAuthenticationError::Unexpected(_))
    ));
}

#[test]
fn firebase_quota_response_is_not_a_terminal_verdict() {
    let result = fetch_access_token_response(
        429,
        r#"{"error":{"code":429,"message":"TOO_MANY_ATTEMPTS_TRY_LATER"}}"#,
    );

    assert!(matches!(
        result,
        Err(UserAuthenticationError::Unexpected(_))
    ));
}

#[test]
fn non_firebase_response_is_not_a_terminal_verdict() {
    let result = fetch_access_token_response(429, "<html>Too many requests</html>");

    assert!(matches!(
        result,
        Err(UserAuthenticationError::Unexpected(_))
    ));
}

#[test]
fn successful_firebase_response_returns_tokens() {
    let tokens = fetch_access_token_response(
        200,
        r#"{"id_token":"new-id-token","refresh_token":"new-refresh-token","expires_in":"3600"}"#,
    )
    .unwrap();

    assert_eq!(tokens.id_token, "new-id-token");
    assert_eq!(tokens.refresh_token, "new-refresh-token");
}

fn exchange_refresh_response(
    status: usize,
    body: &str,
    proxy_requests: usize,
) -> Result<Credentials, UserAuthenticationError> {
    let mut server = mockito::Server::new();
    let direct_request = server
        .mock("POST", "/direct")
        .with_status(status)
        .with_body(body)
        .expect(1)
        .create();
    let proxy_request = server
        .mock("POST", "/proxy")
        .with_status(200)
        .with_body(successful_refresh_response("proxy-id", "proxy-refresh"))
        .expect(proxy_requests)
        .create();
    let (session, _) = session_with_refresh_urls(
        Arc::new(AuthState::new_logged_out_for_test()),
        format!("{}/direct", server.url()),
        format!("{}/proxy", server.url()),
    );

    let result = block_on(session.exchange_credentials(LoginToken::Firebase(
        FirebaseToken::Refresh(RefreshToken::new("refresh-token".to_string())),
    )));

    direct_request.assert();
    proxy_request.assert();
    result
}

#[test]
fn unavailable_firebase_response_falls_back_to_proxy() {
    let result =
        exchange_refresh_response(503, r#"{"error":{"code":503,"message":"UNAVAILABLE"}}"#, 1);

    assert!(matches!(result, Ok(Credentials::Firebase(tokens)) if tokens.id_token == "proxy-id"));
}

#[test]
fn firebase_quota_response_falls_back_to_proxy() {
    let result = exchange_refresh_response(
        429,
        r#"{"error":{"code":429,"message":"TOO_MANY_ATTEMPTS_TRY_LATER"}}"#,
        1,
    );

    assert!(matches!(result, Ok(Credentials::Firebase(tokens)) if tokens.id_token == "proxy-id"));
}

#[test]
fn disabled_account_verdict_does_not_use_proxy() {
    let result = exchange_refresh_response(
        400,
        r#"{"error":{"code":400,"message":"USER_DISABLED"}}"#,
        0,
    );

    assert!(matches!(
        result,
        Err(UserAuthenticationError::UserAccountDisabled(_))
    ));
}

#[test]
fn successful_http_response_without_firebase_payload_falls_back_to_proxy() {
    let result = exchange_refresh_response(200, "not-json", 1);

    assert!(matches!(result, Ok(Credentials::Firebase(tokens)) if tokens.id_token == "proxy-id"));
}
