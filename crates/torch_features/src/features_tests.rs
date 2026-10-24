use super::*;

#[test]
#[ignore = "CORE-3768 - need to clean up PREVIEW_FLAGS, but this is a temporary fix for the cluttered changelog"]
fn test_all_preview_flags_have_a_description() {
    for flag in PREVIEW_FLAGS {
        assert!(
            flag.flag_description()
                .is_some_and(|description| !description.is_empty()),
            "Missing description for preview-enabled flag {flag:?}"
        );
    }
}

#[test]
fn factory_deferred_repositories_are_dogfood_only_by_default() {
    assert!(DOGFOOD_FLAGS.contains(&FeatureFlag::FactoryDeferredRepositories));
    for flags in [DEBUG_FLAGS, LOCAL_FLAGS, PREVIEW_FLAGS, RELEASE_FLAGS] {
        assert!(!flags.contains(&FeatureFlag::FactoryDeferredRepositories));
    }
}

#[test]
fn local_child_harnesses_are_local_only_by_default() {
    assert!(LOCAL_FLAGS.contains(&FeatureFlag::LocalClaudeCodexChildHarnesses));
    assert!(!DEBUG_FLAGS.contains(&FeatureFlag::LocalClaudeCodexChildHarnesses));
    assert!(!DOGFOOD_FLAGS.contains(&FeatureFlag::LocalClaudeCodexChildHarnesses));
}

#[cfg(all(feature = "local_only", feature = "test-util"))]
#[test]
fn torch_cannot_reenable_accounts_ai_or_cloud_with_runtime_overrides() {
    for flag in [
        FeatureFlag::AgentMode,
        FeatureFlag::AgentPredict,
        FeatureFlag::AgentViewPromptChip,
        FeatureFlag::CloudObjects,
        FeatureFlag::CloudConversations,
    ] {
        let _enabled = flag.override_enabled(true);
        assert!(!flag.is_enabled(), "{flag:?} must stay disabled in Torch");
    }
}

/// Torch's updater only reads its own GitHub releases, so release builds may enable it.
#[cfg(all(feature = "local_only", feature = "test-util"))]
#[test]
fn torch_allows_its_own_updater() {
    let _enabled = FeatureFlag::Autoupdate.override_enabled(true);
    assert!(FeatureFlag::Autoupdate.is_enabled());
}

#[cfg(all(feature = "local_only", feature = "test-util"))]
#[test]
fn torch_preserves_normal_completion_preferences() {
    for flag in [
        FeatureFlag::ClassicCompletions,
        FeatureFlag::ForceClassicCompletions,
    ] {
        {
            let _enabled = flag.override_enabled(true);
            assert!(flag.is_enabled());
        }
        let _disabled = flag.override_enabled(false);
        assert!(!flag.is_enabled());
    }
}

#[cfg(all(feature = "local_only", feature = "test-util"))]
#[test]
fn torch_cannot_reenable_file_search_or_editor_features() {
    for flag in [
        FeatureFlag::CommandPaletteFileSearch,
        FeatureFlag::FileTree,
        FeatureFlag::GlobalSearch,
        FeatureFlag::TabbedEditorView,
        FeatureFlag::VimCodeEditor,
        FeatureFlag::CodeFindReplace,
        FeatureFlag::CodeReviewSaveChanges,
        FeatureFlag::AutoOpenCodeReviewPane,
        FeatureFlag::JupyterNotebookRendering,
    ] {
        let _enabled = flag.override_enabled(true);
        assert!(!flag.is_enabled(), "{flag:?} must stay disabled in Torch");
    }
}
