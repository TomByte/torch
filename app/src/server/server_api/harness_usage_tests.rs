use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{TimeZone, Utc};
use futures::executor::block_on;
use serde_json::json;
use torch_harness_usage::api::{
    ClaudeUsage, Coverage, CoverageStatus, HarnessUsageRequest, HarnessUsageSnapshot, ToolCalls,
    UsagePayload, UsageSnapshot,
};

use super::{
    HarnessUsageCapability, HarnessUsageError, HarnessUsageErrorKind, ResolvedHarnessPrompt,
    ServerApi, parse_harness_usage_retry_after,
};
use crate::ai::ambient_agents::AmbientAgentTaskId;

fn task_id() -> AmbientAgentTaskId {
    "550e8400-e29b-41d4-a716-446655440000".parse().unwrap()
}

fn report() -> HarnessUsageRequest {
    HarnessUsageRequest::new(
        41,
        7,
        Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap(),
        HarnessUsageSnapshot::ClaudeCode(UsageSnapshot {
            coverage: Coverage {
                token_status: CoverageStatus::Known,
                tool_status: CoverageStatus::Partial,
            },
            payload: UsagePayload {
                usage: Some(ClaudeUsage {
                    input_tokens: Some(9_007_199_254_740_993),
                    output_tokens: None,
                    cache_read_input_tokens: None,
                    cache_creation_input_tokens: None,
                    cache_creation: None,
                }),
                attribution: Vec::new(),
                tool_calls: Some(ToolCalls {
                    total: 1,
                    by_name: BTreeMap::from([("Read".into(), 1)]),
                }),
            },
        }),
    )
}

#[test]
fn incompatible_startup_preserves_prompt_without_reporting() {
    for value in [
        json!({"prompt": "test"}),
        json!({"prompt": "test", "harness_usage": null}),
        json!({"prompt": "test", "harness_usage": {"execution_id": 0}}),
        json!({"prompt": "test", "harness_usage": {}}),
        json!({"prompt": "test", "harness_usage": "unknown"}),
    ] {
        let prompt: ResolvedHarnessPrompt = serde_json::from_value(value).unwrap();

        assert_eq!(prompt.prompt, "test");
        assert_eq!(prompt.harness_usage, None);
    }
}

#[test]
fn startup_accepts_only_the_advertised_execution() {
    let prompt: ResolvedHarnessPrompt = serde_json::from_value(json!({
        "prompt": "test",
        "resumption_prompt": "continue",
        "harness_usage": {"execution_id": 41, "future_field": true}
    }))
    .unwrap();

    assert_eq!(
        prompt.harness_usage,
        Some(HarnessUsageCapability { execution_id: 41 })
    );
    assert_eq!(prompt.resumption_prompt.as_deref(), Some("continue"));
}

#[test]
fn invalid_reports_are_rejected_before_auth_or_network() {
    for (execution_id, capture_sequence) in [(41, 0), (0, 7)] {
        let mut report = report();
        report.execution_id = execution_id;
        report.capture_sequence = capture_sequence;

        let error =
            block_on(ServerApi::new_for_test().publish_harness_usage_for_task(&task_id(), &report))
                .unwrap_err();

        assert_eq!(error.kind, HarnessUsageErrorKind::InvalidReport);
        assert_eq!(error.status, None);
    }
}

#[test]
fn retry_after_http_date_uses_response_time_not_capture_time() {
    let now = Utc.with_ymd_and_hms(2026, 9, 10, 12, 0, 0).unwrap();

    assert_eq!(
        parse_harness_usage_retry_after("Thu, 10 Sep 2026 12:00:02 GMT", now),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        parse_harness_usage_retry_after("Thu, 10 Sep 2026 11:59:00 GMT", now),
        Some(Duration::ZERO)
    );
    assert_eq!(parse_harness_usage_retry_after("invalid", now), None);
}

#[test]
fn request_preparation_errors_are_retryable_without_retaining_diagnostics() {
    let error =
        HarnessUsageError::from_request_preparation_error(anyhow::anyhow!("private diagnostic"));

    assert_eq!(error.kind, HarnessUsageErrorKind::Retryable);
    assert!(!format!("{error:?}").contains("private diagnostic"));
}
