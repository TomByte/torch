use std::collections::HashMap;

use warp_multi_agent_api as api;

use super::{ChargedUsageTotals, ConversationUsageMetadata, ModelTokenUsage};

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

fn inference_usage_with_web_search(
    input: u64,
    output: u64,
    input_cost_in_cents: f32,
    output_cost_in_cents: f32,
    web_search_count: u32,
    web_search_cost_in_cents: f32,
) -> api::InferenceUsage {
    api::InferenceUsage {
        token_count: Some(api::TokenCount {
            input,
            output,
            ..Default::default()
        }),
        token_cost: Some(api::TokenCost {
            input_cost_in_cents,
            output_cost_in_cents,
            ..Default::default()
        }),
        web_search_count,
        web_search_cost_in_cents,
        ..Default::default()
    }
}

#[test]
fn charged_usage_totals_sums_web_search_fields_across_categories_and_models() {
    let mut usage_by_category = HashMap::new();
    usage_by_category.insert(
        "primary_agent".to_string(),
        api::ChargedUsage {
            direct_api_inference_usage: HashMap::from([(
                "claude-4.5".to_string(),
                inference_usage_with_web_search(1000, 200, 3.0, 6.0, 2, 5.0),
            )]),
            platform_usage_in_cents: 1.0,
            ..Default::default()
        },
    );
    usage_by_category.insert(
        "compaction".to_string(),
        api::ChargedUsage {
            direct_api_inference_usage: HashMap::from([(
                "claude-4.5".to_string(),
                inference_usage_with_web_search(500, 100, 1.5, 3.0, 1, 2.5),
            )]),
            platform_usage_in_cents: 0.0,
            ..Default::default()
        },
    );
    let charges = api::RequestCharges { usage_by_category };

    let totals = ChargedUsageTotals::from(&charges);

    assert_eq!(totals.web_search_count, 3);
    assert!((totals.web_search_cost_in_cents - 7.5).abs() < 1e-6);
    // Total cost must include web search cost alongside the token + platform costs.
    assert!((totals.total_cost_in_cents() - (3.0 + 6.0 + 1.5 + 3.0 + 1.0 + 7.5)).abs() < 1e-6);
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
fn charged_usage_totals_sums_credit_fields() {
    let charges = api::RequestCharges {
        usage_by_category: HashMap::from([(
            "primary_agent".to_string(),
            api::ChargedUsage {
                direct_api_inference_usage: HashMap::from([(
                    "claude-4.5".to_string(),
                    api::InferenceUsage {
                        token_count: None,
                        token_cost: Some(api::TokenCost {
                            input_cost_in_credits: 1.0,
                            output_cost_in_credits: 2.0,
                            input_cache_read_cost_in_credits: 3.0,
                            input_cache_write_cost_in_credits: 4.0,
                            ..Default::default()
                        }),
                        web_search_cost_in_credits: 5.0,
                        ..Default::default()
                    },
                )]),
                platform_usage_in_credits: 6.0,
                ..Default::default()
            },
        )]),
    };

    let totals = ChargedUsageTotals::from(&charges);

    assert_eq!(totals.input_cost_in_credits, 1.0);
    assert_eq!(totals.output_cost_in_credits, 2.0);
    assert_eq!(totals.input_cache_read_cost_in_credits, 3.0);
    assert_eq!(totals.input_cache_write_cost_in_credits, 4.0);
    assert_eq!(totals.web_search_cost_in_credits, 5.0);
    assert_eq!(totals.platform_cost_in_credits, 6.0);
    assert_eq!(totals.total_cost_in_credits(), 21.0);
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

#[allow(deprecated)]
#[test]
fn model_token_usage_replays_custom_endpoint_usage_by_model_id() {
    let usage = ModelTokenUsage {
        model_id: "Friendly alias".to_string(),
        custom_endpoint_tokens: 6,
        custom_endpoint_token_usage_by_category: HashMap::from([("primary_agent".to_string(), 6)]),
        ..Default::default()
    };

    let (key, proto) = usage
        .to_proto_custom_endpoint_usage()
        .expect("custom endpoint usage should serialize for replay");

    assert_eq!(key, "Friendly alias");
    assert_eq!(proto.model_id, "Friendly alias");
    assert_eq!(proto.total_tokens, 6);
    assert_eq!(proto.token_usage_by_category.get("primary_agent"), Some(&6));
}

#[allow(deprecated)]
#[test]
fn model_token_usage_replay_skips_non_custom_endpoint_entries() {
    let warp_only = ModelTokenUsage {
        model_id: "warp-model".to_string(),
        warp_tokens: 4,
        ..Default::default()
    };
    assert!(warp_only.to_proto_custom_endpoint_usage().is_none());
}

#[test]
fn charged_usage_totals_saturates_wide_wire_counts_and_cumulative_totals() {
    let charges = api::RequestCharges {
        usage_by_category: HashMap::from([(
            "primary_agent".to_string(),
            api::ChargedUsage {
                direct_api_inference_usage: HashMap::from([(
                    "model".to_string(),
                    api::InferenceUsage {
                        token_count: Some(api::TokenCount {
                            input: u64::MAX,
                            output: u64::from(u32::MAX) + 1,
                            input_cache_read: u64::from(u32::MAX),
                            input_cache_write: 1,
                        }),
                        ..Default::default()
                    },
                )]),
                ..Default::default()
            },
        )]),
    };

    let mut totals = ChargedUsageTotals::from(&charges);
    assert_eq!(totals.input_tokens, u32::MAX);
    assert_eq!(totals.output_tokens, u32::MAX);
    assert_eq!(totals.input_cache_read_tokens, u32::MAX);
    assert_eq!(totals.input_cache_write_tokens, 1);
    assert_eq!(totals.total_tokens(), u32::MAX);

    totals += totals;
    assert_eq!(totals.input_tokens, u32::MAX);
    assert_eq!(totals.output_tokens, u32::MAX);
    assert_eq!(totals.input_cache_read_tokens, u32::MAX);
    assert_eq!(totals.input_cache_write_tokens, 2);
    assert_eq!(totals.total_tokens(), u32::MAX);
}
