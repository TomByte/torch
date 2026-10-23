use std::collections::HashMap;
use std::pin::pin;
use std::sync::Arc;
use std::task::Poll;

use chrono::{Duration, Utc};
use futures::channel::oneshot;
use futures::executor::block_on;
use futures::{join, poll};
use torch_isolation_platform::{IsolationPlatformError, WorkloadToken};
use torch_server_auth::auth_state::AuthState;

use super::{
    AGENT_SOURCE_HEADER, AMBIENT_WORKLOAD_TOKEN_DURATION, AMBIENT_WORKLOAD_TOKEN_HEADER,
    AmbientHeaderPolicy, AuthenticatedGraphqlConfig, BaseClient, CLOUD_AGENT_ID_HEADER,
    GraphqlRoutingConfig, HeaderOverride,
};

struct StaticIapTokenProvider;

impl http_client::iap::IapTokenProvider for StaticIapTokenProvider {
    fn cached_token(&self) -> Option<String> {
        Some("iap-token".to_string())
    }
}

fn client() -> BaseClient {
    let (event_sender, _) = async_channel::unbounded();
    let mut authenticated_headers = HashMap::new();
    authenticated_headers.insert("X-Test-Authenticated".to_string(), "true".to_string());
    BaseClient::new(
        Arc::new(http_client::Client::new()),
        Arc::new(http_client::Client::new_external()),
        Arc::new(AuthState::new_for_test()),
        event_sender,
        Some("cloud_mode".to_string()),
        GraphqlRoutingConfig {
            path_prefix: Some("/routing-only".to_string()),
        },
        AuthenticatedGraphqlConfig {
            headers: authenticated_headers,
        },
        None,
    )
}

#[test]
fn iap_proxy_auth_header_uses_configured_provider() {
    let (event_sender, _) = async_channel::unbounded();
    let client = BaseClient::new(
        Arc::new(http_client::Client::new()),
        Arc::new(http_client::Client::new_external()),
        Arc::new(AuthState::new_for_test()),
        event_sender,
        None,
        GraphqlRoutingConfig::default(),
        AuthenticatedGraphqlConfig::default(),
        Some(Arc::new(StaticIapTokenProvider)),
    );

    assert_eq!(
        client.iap_proxy_auth_header(),
        Some((
            http_client::iap::IAP_PROXY_AUTH_HEADER,
            "Bearer iap-token".to_string()
        ))
    );
}

#[test]
fn explicit_token_graphql_options_route_without_authenticated_headers() {
    let client = client();
    client.set_ambient_agent_task_id(Some("ambient-task".to_string()));

    let options = client.graphql_request_options_with_token(Some("token".to_string()));

    assert_eq!(options.path_prefix.as_deref(), Some("/routing-only"));
    assert_eq!(options.auth_token.as_deref(), Some("token"));
    assert!(options.headers.is_empty());
}

#[test]
fn ambient_policy_supports_inherit_override_and_omit() {
    let client = client();
    client.set_ambient_agent_task_id(Some("ambient-task".to_string()));

    let inherited = block_on(client.ambient_headers(AmbientHeaderPolicy {
        workload_token: HeaderOverride::Set("workload".to_string()),
        cloud_agent_id: HeaderOverride::Inherit,
        agent_source: HeaderOverride::Inherit,
    }))
    .unwrap();
    assert!(inherited.contains(&(
        AMBIENT_WORKLOAD_TOKEN_HEADER.to_string(),
        "workload".to_string(),
    )));
    assert!(inherited.contains(&(
        CLOUD_AGENT_ID_HEADER.to_string(),
        "ambient-task".to_string()
    )));
    assert!(inherited.contains(&(AGENT_SOURCE_HEADER.to_string(), "cloud_mode".to_string())));

    let task_scoped = block_on(client.ambient_headers(AmbientHeaderPolicy {
        workload_token: HeaderOverride::Set("workload".to_string()),
        ..AmbientHeaderPolicy::for_task("specific-task")
    }))
    .unwrap();
    assert!(task_scoped.contains(&(
        CLOUD_AGENT_ID_HEADER.to_string(),
        "specific-task".to_string(),
    )));
    assert!(!task_scoped.contains(&(
        CLOUD_AGENT_ID_HEADER.to_string(),
        "ambient-task".to_string()
    )));

    let omitted = block_on(client.ambient_headers(AmbientHeaderPolicy::omit_all())).unwrap();
    assert!(omitted.is_empty());
}

#[test]
fn pinned_workload_token_is_withheld_unless_it_outlives_the_deadline() {
    let client = client();
    let deadline = chrono::Utc::now() + chrono::Duration::hours(6);

    client.set_ambient_workload_token_for_test(
        "long-lived".to_string(),
        Some(deadline + chrono::Duration::hours(1)),
    );
    assert_eq!(
        block_on(client.get_ambient_workload_token_valid_until(deadline)).unwrap(),
        Some("long-lived".to_string()),
    );

    // A token that expires mid-run must never be handed out for pinning, even though it is
    // valid right now. Asserted as an inequality rather than `None` because a
    // `TORCH_WORKLOAD_TOKEN` in the ambient environment legitimately yields a non-expiring
    // replacement here.
    client.set_ambient_workload_token_for_test(
        "expires-mid-run".to_string(),
        Some(deadline - chrono::Duration::hours(1)),
    );
    assert_ne!(
        block_on(client.get_ambient_workload_token_valid_until(deadline)).unwrap(),
        Some("expires-mid-run".to_string()),
    );

    // Platforms that issue non-expiring tokens outlive every deadline.
    client.set_ambient_workload_token_for_test("never-expires".to_string(), None);
    assert_eq!(
        block_on(client.get_ambient_workload_token_valid_until(deadline)).unwrap(),
        Some("never-expires".to_string()),
    );
}

#[test]
fn concurrent_workload_token_requests_with_cold_cache_share_a_refresh() {
    assert_concurrent_workload_token_requests_share_a_refresh(client());
}

#[test]
fn concurrent_workload_token_requests_with_expired_cache_share_a_refresh() {
    let client = client();
    client.set_ambient_workload_token_for_test(
        "expired".to_string(),
        Some(Utc::now() - Duration::seconds(1)),
    );
    assert_concurrent_workload_token_requests_share_a_refresh(client);
}

fn assert_concurrent_workload_token_requests_share_a_refresh(client: BaseClient) {
    block_on(async {
        let valid_until = Utc::now() + Duration::minutes(5);
        let (release, wait) = oneshot::channel();
        let mut first = pin!(client.workload_token_valid_until_with(
            valid_until,
            AMBIENT_WORKLOAD_TOKEN_DURATION,
            |duration| async move {
                assert_eq!(duration, Some(AMBIENT_WORKLOAD_TOKEN_DURATION));
                wait.await.unwrap();
                Ok(WorkloadToken {
                    token: "refreshed".to_string(),
                    expires_at: Some(valid_until + Duration::hours(1)),
                })
            },
        ));
        let mut second = pin!(client.workload_token_valid_until_with(
            valid_until,
            AMBIENT_WORKLOAD_TOKEN_DURATION,
            |_| async { panic!("a concurrent caller must reuse the refreshed token") },
        ));

        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());
        release.send(()).unwrap();

        let (first, second) = join!(first, second);
        assert_eq!(first.unwrap().unwrap().token, "refreshed");
        assert_eq!(second.unwrap().unwrap().token, "refreshed");
    });
}

#[test]
fn workload_token_refresh_failure_releases_waiting_caller() {
    block_on(async {
        let client = client();
        let valid_until = Utc::now() + Duration::minutes(5);
        let (release, wait) = oneshot::channel();
        let mut first = pin!(client.workload_token_valid_until_with(
            valid_until,
            AMBIENT_WORKLOAD_TOKEN_DURATION,
            |_| async {
                wait.await.unwrap();
                Err(IsolationPlatformError::GenericWorkloadTokenMissing)
            },
        ));
        let mut second = pin!(client.workload_token_valid_until_with(
            valid_until,
            AMBIENT_WORKLOAD_TOKEN_DURATION,
            |_| async {
                Ok(WorkloadToken {
                    token: "next-attempt".to_string(),
                    expires_at: None,
                })
            },
        ));

        assert!(poll!(&mut first).is_pending());
        assert!(poll!(&mut second).is_pending());
        release.send(()).unwrap();

        let (first, second) = join!(first, second);
        assert!(first.is_err());
        assert_eq!(second.unwrap().unwrap().token, "next-attempt");
    });
}

#[test]
fn cached_workload_token_does_not_wait_for_refresh() {
    block_on(async {
        let client = client();
        let _refresh_guard = client.ambient_workload_token_refresh.lock().await;
        client.set_ambient_workload_token_for_test("cached".to_string(), None);
        let mut request = pin!(client.get_or_create_ambient_workload_token());

        assert_eq!(
            poll!(&mut request).map(Result::unwrap),
            Poll::Ready(Some("cached".to_string())),
        );
    });
}
