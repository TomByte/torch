use super::{ChargedUsageTotals, ConversationUsageMetadata};

/// Conversations persisted before the provider cost was dropped still carry it; the blob must
/// keep deserializing and the stale key must not be written back.
#[test]
fn conversation_usage_metadata_ignores_a_persisted_provider_cost() {
    let metadata: ConversationUsageMetadata = serde_json::from_str(
        r#"{"was_summarized":false,"context_window_usage":0.0,"credits_spent":1.5,"total_provider_cost_in_cents":3.2}"#,
    )
    .unwrap();

    assert_eq!(metadata.credits_spent, 1.5);
    assert_eq!(metadata.total_billed_cost_in_cents, None);
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("total_provider_cost_in_cents")
    );
}

#[test]
fn conversation_usage_metadata_defaults_missing_billed_cost_to_unknown() {
    let metadata: ConversationUsageMetadata = serde_json::from_str(
        r#"{"was_summarized":false,"context_window_usage":0.0,"credits_spent":0.0}"#,
    )
    .unwrap();

    assert_eq!(metadata.total_billed_cost_in_cents, None);
    assert_eq!(metadata.billed_cost_in_cents(), None);
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("total_billed_cost_in_cents")
    );
}

#[test]
fn billed_cost_prefers_summed_charges_over_graphql_total() {
    let metadata = ConversationUsageMetadata {
        total_billed_cost_in_cents: Some(12.0),
        total_charged_usage: Some(ChargedUsageTotals {
            input_cost_in_cents: 3.0,
            output_cost_in_cents: 4.0,
            platform_cost_in_cents: 1.0,
            web_search_cost_in_cents: 0.5,
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(metadata.billed_cost_in_cents(), Some(8.5));
}

#[test]
fn billed_cost_falls_back_to_graphql_total_without_summed_charges() {
    let metadata = ConversationUsageMetadata {
        total_billed_cost_in_cents: Some(12.0),
        ..Default::default()
    };

    assert_eq!(metadata.billed_cost_in_cents(), Some(12.0));
}

#[test]
fn charged_usage_totals_add_assign_sums_web_search_fields() {
    let mut a = ChargedUsageTotals {
        web_search_count: 2,
        web_search_cost_in_cents: 4.0,
        ..Default::default()
    };
    let b = ChargedUsageTotals {
        web_search_count: 3,
        web_search_cost_in_cents: 6.0,
        ..Default::default()
    };

    a += b;

    assert_eq!(a.web_search_count, 5);
    assert!((a.web_search_cost_in_cents - 10.0).abs() < 1e-6);
}

#[test]
fn charged_usage_totals_deserializes_legacy_payload_without_web_search_fields() {
    let totals: ChargedUsageTotals = serde_json::from_str(
        r#"{"input_cost_in_cents":1.0,"output_cost_in_cents":2.0,"input_cache_read_cost_in_cents":0.0,"input_cache_write_cost_in_cents":0.0,"platform_cost_in_cents":0.0,"input_tokens":10,"output_tokens":5,"input_cache_read_tokens":0,"input_cache_write_tokens":0}"#,
    )
    .unwrap();

    assert_eq!(totals.web_search_count, 0);
    assert_eq!(totals.web_search_cost_in_cents, 0.0);
    assert_eq!(totals.total_cost_in_credits(), 0.0);
    assert!((totals.total_cost_in_cents() - 3.0).abs() < 1e-6);
}

#[test]
fn charged_usage_totals_add_assign_saturates_token_counts() {
    let mut totals = ChargedUsageTotals {
        input_tokens: u32::MAX,
        input_cache_write_tokens: 1,
        ..Default::default()
    };

    totals += totals;

    assert_eq!(totals.input_tokens, u32::MAX);
    assert_eq!(totals.input_cache_write_tokens, 2);
    assert_eq!(totals.total_tokens(), u32::MAX);
}
