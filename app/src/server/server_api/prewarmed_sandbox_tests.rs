use torch_graphql::queries::execution_config::{CodeForge, ExecutionRepositoryRefType};

use super::*;

const CONFIGURATION_JSON: &str = include_str!(
    "../../../../crates/graphql/src/api/mutations/fixtures/prewarmed_sandbox_config.json"
);

#[test]
#[cfg(not(target_family = "wasm"))]
fn control_unavailable_recognizes_native_handshake_410() {
    for status in [410, 403, 503] {
        let response = websocket::tungstenite::http::Response::builder()
            .status(status)
            .body(None)
            .unwrap();
        let error = anyhow::Error::new(websocket::tungstenite::Error::Http(response))
            .context("control handshake failed");
        assert_eq!(is_prewarmed_sandbox_unavailable(&error), status == 410);
    }
}

#[test]
fn control_unavailable_recognizes_an_http_410() {
    for status in [410, 403, 503] {
        let error = anyhow::Error::new(HttpStatusError::new(status, String::new()))
            .context("workload token request failed");
        assert_eq!(is_prewarmed_sandbox_unavailable(&error), status == 410);
    }
}

#[test]
fn control_socket_url_uses_websocket_schemes_and_binds_the_generation() {
    for (http_url, expected) in [
        (
            "https://app.warp.dev/internal/prewarmed-sandboxes/sb/ws",
            "wss://app.warp.dev/internal/prewarmed-sandboxes/sb/ws?generation=abc123",
        ),
        (
            "http://localhost:8080/internal/prewarmed-sandboxes/sb/ws",
            "ws://localhost:8080/internal/prewarmed-sandboxes/sb/ws?generation=abc123",
        ),
    ] {
        assert_eq!(control_socket_url(http_url, "abc123").unwrap(), expected);
    }
    assert!(control_socket_url("ftp://app.warp.dev/ws", "abc123").is_err());
}

#[test]
#[cfg(not(target_family = "wasm"))]
fn native_control_headers_are_sensitive() {
    let request =
        control_request("ws://localhost/control", "access-marker", "workload-marker").unwrap();
    for name in ["authorization", PREWARMED_SANDBOX_WORKLOAD_TOKEN_HEADER] {
        assert!(request.headers()[name].is_sensitive());
    }
    for debug in [format!("{request:?}"), format!("{request:#?}")] {
        assert!(!debug.contains("access-marker"));
        assert!(!debug.contains("workload-marker"));
    }
}

fn wire_configuration() -> GraphqlConfiguration {
    let response: cynic::GraphQlResponse<PrewarmedSandboxConfig> =
        serde_json::from_str(CONFIGURATION_JSON).unwrap();
    let PrewarmedSandboxConfigResult::PrewarmedSandboxConfiguration(config) =
        response.data.unwrap().prewarmed_sandbox_config
    else {
        panic!("expected a configuration");
    };
    *config
}

pub(crate) fn parse_configuration() -> PrewarmedSandboxConfiguration {
    wire_configuration().try_into().unwrap()
}

#[test]
fn deserializes_a_server_configuration() {
    let config = parse_configuration();

    assert_eq!(config.generation, "abc123");
    assert_eq!(config.preparation.harness, "oz");

    let repository = &config.preparation.repositories[0];
    assert_eq!(repository.forge, CodeForge::GitHub);
    assert_eq!(repository.owner, "warpdotdev");
    assert_eq!(repository.name, "warp");
    assert_eq!(
        repository.ref_.as_ref().unwrap().type_,
        ExecutionRepositoryRefType::Branch
    );
    assert_eq!(repository.ref_.as_ref().unwrap().value, "main");
    assert_eq!(
        repository
            .clone_from
            .as_ref()
            .map(|repo| repo.repo.as_str()),
        Some("warp-fork")
    );
    assert!(repository.preserve_origin);

    let deferred = &config.preparation.deferred_repositories[0];
    assert_eq!(deferred.code_forge, CodeForge::GitHub);
    assert_eq!(deferred.owner, "warpdotdev");
    assert_eq!(deferred.repo, "warp-server");

    assert_eq!(config.preparation.setup_commands, ["./script/bootstrap"]);
    assert!(config.preparation.mcp_servers.contains_key("linear"));
    assert_eq!(config.preparation.skills, ["warpdotdev/warp:review"]);
    assert_eq!(config.preparation.factory_skill_dirs, [".agents/skills"]);
    assert!(config.preparation.computer_use_enabled);

    assert_eq!(
        config.environment_variables.get("TORCH_FACTORY_REPO_DIR"),
        Some(&"factory".to_string())
    );
    assert_eq!(config.git_credentials[0].host, "github.com");
    assert_eq!(config.git_credentials[0].token, "ghs_secret");
    assert_eq!(
        config.git_credentials[0].username.as_deref(),
        Some("x-access-token")
    );
    assert!(config.git_credentials[0].email.is_none());
    assert!(matches!(
        config.managed_secrets.get("API_TOKEN"),
        Some(ManagedSecretValue::RawValue { value }) if value == "value"
    ));
    assert!(matches!(
        config.managed_secrets.get("CLAUDE_AUTH"),
        Some(ManagedSecretValue::AnthropicApiKey { api_key }) if api_key == "sk-ant-key"
    ));
}

#[test]
fn converts_empty_configuration_lists() {
    let mut config = wire_configuration();
    config.spec.repositories.clear();
    config.spec.deferred_repositories.clear();
    config.spec.setup_commands.clear();
    config.spec.mcp_servers_json = "{}".into();
    config.spec.skills.clear();
    config.spec.factory_skill_dirs.clear();
    config.environment_variables.clear();
    config.git_credentials.clear();
    config.managed_secrets.clear();

    let config = PrewarmedSandboxConfiguration::try_from(config).unwrap();
    assert!(config.preparation.repositories.is_empty());
    assert!(config.preparation.deferred_repositories.is_empty());
    assert!(config.preparation.mcp_servers.is_empty());
    assert!(config.environment_variables.is_empty());
    assert!(config.git_credentials.is_empty());
    assert!(config.managed_secrets.is_empty());
    assert_eq!(config.expires_at.to_rfc3339(), "2026-01-01T00:00:00+00:00");
}

#[test]
fn unknown_managed_secret_types_fail_conversion() {
    let mut config = wire_configuration();
    config.managed_secrets[0].value = serde_json::from_value(serde_json::json!({
        "__typename": "ManagedSecretFutureValue"
    }))
    .unwrap();
    let error = PrewarmedSandboxConfiguration::try_from(config).unwrap_err();
    assert_eq!(
        error.to_string(),
        "Invalid pre-warmed sandbox managed secrets"
    );
}

#[test]
fn invalid_mcp_json_errors_do_not_include_configuration_values() {
    for json in ["invalid-secret-json", "\"secret-value\"", "[]", "null"] {
        let mut config = wire_configuration();
        config.spec.mcp_servers_json = json.into();
        let error = PrewarmedSandboxConfiguration::try_from(config).unwrap_err();
        assert_eq!(
            format!("{error:#}"),
            "Invalid pre-warmed sandbox MCP server configuration"
        );
    }
}

#[test]
fn configuration_debug_redacts_credentials_and_raw_preparation_values() {
    let mut config = parse_configuration();
    config.managed_secrets.insert(
        "TOKEN".into(),
        ManagedSecretValue::raw_value("managed-secret-value"),
    );
    config.environment_variables.insert(
        "TORCH_FACTORY_REPO_CLONE_URL".into(),
        "https://environment-secret-value@example.com/repo.git".into(),
    );
    config.git_credentials[0].token = "git-secret-value".into();
    config.preparation.setup_commands = vec!["login --token setup-secret-value".into()];
    config.preparation.mcp_servers = serde_json::from_value(serde_json::json!({
        "local": {
            "command": "mcp",
            "args": ["--token", "mcp-argument-secret-value"],
            "env": {"TOKEN": "mcp-environment-secret-value"}
        },
        "remote": {
            "url": "https://example.com/mcp",
            "headers": {"Authorization": "Bearer mcp-header-secret-value"}
        }
    }))
    .unwrap();

    for output in [
        format!("{config:?}"),
        format!("{config:#?}"),
        format!("{:?}", config.preparation),
        format!("{:#?}", config.preparation),
    ] {
        for secret in [
            "managed-secret-value",
            "sk-ant-key",
            "environment-secret-value",
            "git-secret-value",
            "setup-secret-value",
            "mcp-argument-secret-value",
            "mcp-environment-secret-value",
            "mcp-header-secret-value",
        ] {
            assert!(!output.contains(secret), "Debug output exposed {secret}");
        }
        assert!(output.contains("mcp_server_count"));
        assert!(output.contains("computer_use_enabled"));
    }
    let output = format!("{config:?}");
    assert!(output.contains("abc123"));
    assert!(output.contains("managed_secret_count: 3"));
    assert!(output.contains("git_credential_count: 1"));
}

#[test]
fn ready_status_sends_the_fingerprint_as_the_manifest_hash() {
    let ready = PrewarmedSandboxStatusInput {
        fingerprint: "abc123".to_string(),
        state: PrewarmedSandboxState::Ready.as_wire_value(),
        manifest_hash: Some("abc123".to_string()),
    };
    assert_eq!(
        serde_json::to_value(&ready).unwrap(),
        serde_json::json!({
            "fingerprint": "abc123",
            "state": "ready",
            "manifest_hash": "abc123",
        })
    );

    let failed = PrewarmedSandboxStatusInput {
        fingerprint: "abc123".to_string(),
        state: PrewarmedSandboxState::Failed.as_wire_value(),
        manifest_hash: None,
    };
    assert_eq!(
        serde_json::to_value(&failed).unwrap(),
        serde_json::json!({ "fingerprint": "abc123", "state": "failed" })
    );
}

#[test]
fn heartbeat_is_the_only_sandbox_message() {
    assert_eq!(
        serde_json::to_value(PrewarmedSandboxControlRequest::Heartbeat {
            generation: "abc123"
        })
        .unwrap(),
        serde_json::json!({ "type": "heartbeat", "generation": "abc123" })
    );
}

#[test]
fn decodes_retirement_with_and_without_a_reason() {
    for (message, expected) in [
        (
            r#"{"type":"retired","reason":"unavailable"}"#,
            Some("unavailable"),
        ),
        (r#"{"type":"retired"}"#, None),
    ] {
        let event: PrewarmedSandboxControlEvent = serde_json::from_str(message).unwrap();
        let PrewarmedSandboxControlEvent::Retired { reason } = event else {
            panic!("expected retirement");
        };
        assert_eq!(reason.as_deref(), expected);
    }
}

/// A server can add message types without breaking a sandbox that is already waiting.
#[test]
fn decodes_messages_this_client_does_not_know_as_unknown() {
    for message in [
        r#"{"type":"offer"}"#,
        r#"{"type":"configuration_updated","generation":"def456"}"#,
    ] {
        let event: PrewarmedSandboxControlEvent = serde_json::from_str(message).unwrap();
        assert!(matches!(event, PrewarmedSandboxControlEvent::Unknown));
    }
}

/// The server pings an idle socket, and a ping must not end the connection.
#[test]
fn control_reader_skips_non_text_frames() {
    let mut reader = PrewarmedSandboxControlReader {
        stream: Box::pin(futures::stream::iter([
            Ok::<_, websocket::Error>(websocket::Message::new_binary(vec![0])),
            Ok(websocket::Message::new_text(
                r#"{"type":"retired","reason":"unavailable"}"#.into(),
            )),
        ])),
    };
    let Some(PrewarmedSandboxControlEvent::Retired { reason }) =
        futures::executor::block_on(reader.next()).unwrap()
    else {
        panic!("expected retirement");
    };
    assert_eq!(reason.as_deref(), Some("unavailable"));
}

#[test]
fn bounds_a_stalled_request() {
    let error = futures::executor::block_on(bounded_request(
        Duration::ZERO,
        futures::future::pending::<anyhow::Result<()>>(),
    ))
    .unwrap_err();
    assert_eq!(
        error.downcast_ref::<std::io::Error>().unwrap().kind(),
        std::io::ErrorKind::TimedOut,
    );
}

#[test]
fn request_deadline_preserves_completed_results() {
    let value =
        futures::executor::block_on(bounded_request(REQUEST_TIMEOUT, async { Ok(42) })).unwrap();
    assert_eq!(value, 42);
    let error = futures::executor::block_on(bounded_request::<()>(REQUEST_TIMEOUT, async {
        anyhow::bail!("credential failure")
    }))
    .unwrap_err();
    assert_eq!(error.to_string(), "credential failure");
}

#[test]
fn fresh_assignment_fixture_decodes_nullable_control_fields() {
    let fixture = include_str!("testdata/prewarmed_assignment_fresh_run.json");
    let event: PrewarmedSandboxControlEvent = serde_json::from_str(fixture).unwrap();
    let PrewarmedSandboxControlEvent::Assignment {
        generation,
        execution_config,
        runtime_env,
        api_key,
        expires_at,
    } = event
    else {
        panic!("expected assignment event");
    };
    assert_eq!(generation, "generation-abc");
    assert_eq!(
        execution_config.task_id.inner(),
        "550e8400-e29b-41d4-a716-446655440000"
    );
    assert_eq!(execution_config.execution_id.inner(), "42");
    assert_eq!(runtime_env["TORCH_SANDBOX_DEADLINE"], "1767225600");
    assert!(execution_config.conversation_id.is_none());
    assert!(execution_config.repositories[0].ref_.is_none());
    assert!(
        execution_config
            .inference_providers
            .as_ref()
            .unwrap()
            .aws_bedrock
            .as_ref()
            .unwrap()
            .region
            .is_none()
    );
    assert_eq!(api_key, "wk-placeholder-not-a-real-key");
    assert_eq!(expires_at.to_rfc3339(), "2026-01-01T00:00:00+00:00");
}

#[test]
fn assignment_debug_omits_run_credentials_and_embedded_config_values() {
    let mut event: PrewarmedSandboxControlEvent =
        serde_json::from_str(include_str!("testdata/prewarmed_assignment_fresh_run.json")).unwrap();
    if let PrewarmedSandboxControlEvent::Assignment {
        api_key,
        runtime_env,
        execution_config,
        ..
    } = &mut event
    {
        *api_key = "assignment-key-marker".into();
        runtime_env.insert("SECRET".into(), "assignment-env-marker".into());
        execution_config.mcp_servers_json = "assignment-mcp-marker".into();
        execution_config.setup_commands = vec!["assignment-setup-marker".into()];
    }
    for debug in [format!("{event:?}"), format!("{event:#?}")] {
        for marker in [
            "assignment-key-marker",
            "assignment-env-marker",
            "assignment-mcp-marker",
            "assignment-setup-marker",
        ] {
            assert!(!debug.contains(marker));
        }
    }
}
