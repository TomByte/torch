use std::collections::HashMap;

use chrono::Local;
use smol_str::SmolStr;
use torch_editor::render::model::LineCount;
use torch_util::path::EscapeChar;

use super::{
    CLIAgent, build_diff_hunk_prompt, build_review_prompt, build_selection_line_range_prompt,
    build_selection_substring_prompt,
};
use crate::agent_status::{AgentReviewCommentBatch, DiffSetHunk};
use crate::code::buffer_location::LocalOrRemotePath;
use crate::code::editor::line::EditorLineLocation;
use crate::code_review::comments::{
    AttachedReviewComment, AttachedReviewCommentTarget, LineDiffContent,
};
use crate::ui_components::icons::Icon;

/// Helper to build an alias map from pairs.
fn aliases(pairs: &[(&str, &str)]) -> HashMap<SmolStr, String> {
    pairs
        .iter()
        .map(|(k, v)| (SmolStr::new(k), v.to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers for prompt-building tests
// ---------------------------------------------------------------------------

fn make_comment(
    content: &str,
    target: AttachedReviewCommentTarget,
    outdated: bool,
) -> AttachedReviewComment {
    AttachedReviewComment {
        id: Default::default(),
        content: content.to_string(),
        target,
        last_update_time: Local::now(),
        outdated,
    }
}

fn batch(comments: Vec<AttachedReviewComment>) -> AgentReviewCommentBatch {
    AgentReviewCommentBatch {
        comments,
        diff_set: HashMap::new(),
    }
}

fn local_path(path: &str) -> LocalOrRemotePath {
    LocalOrRemotePath::Local(path.into())
}

// ---------------------------------------------------------------------------
// build_review_prompt tests
// ---------------------------------------------------------------------------

#[test]
fn test_build_review_prompt_current_line_is_1_indexed() {
    // LineCount 0 (0-indexed) should appear as L1 in the prompt.
    let comment = make_comment(
        "fix this",
        AttachedReviewCommentTarget::Line {
            absolute_file_path: local_path("/repo/src/main.rs"),
            line: EditorLineLocation::Current {
                line_number: LineCount::from(0),
                line_range: LineCount::from(0)..LineCount::from(1),
            },
            content: LineDiffContent::default(),
        },
        false,
    );
    let prompt = build_review_prompt(&batch(vec![comment]));
    assert!(
        prompt.contains("/repo/src/main.rs L1"),
        "expected 1-indexed L1, got: {prompt}",
    );
    assert!(prompt.contains("fix this"));
}

#[test]
fn test_build_review_prompt_removed_line_is_1_indexed() {
    let comment = make_comment(
        "why was this deleted?",
        AttachedReviewCommentTarget::Line {
            absolute_file_path: local_path("/repo/old.rs"),
            line: EditorLineLocation::Removed {
                line_number: LineCount::from(9),
                line_range: LineCount::from(9)..LineCount::from(10),
                index: 0,
            },
            content: LineDiffContent::default(),
        },
        false,
    );
    let prompt = build_review_prompt(&batch(vec![comment]));
    assert!(
        prompt.contains("(deleted, was L10"),
        "expected 1-indexed L10, got: {prompt}",
    );
}

#[test]
fn test_build_review_prompt_collapsed_range_is_1_indexed_start() {
    let comment = make_comment(
        "check this hunk",
        AttachedReviewCommentTarget::Line {
            absolute_file_path: local_path("/repo/lib.rs"),
            line: EditorLineLocation::Collapsed {
                line_range: LineCount::from(4)..LineCount::from(10),
            },
            content: LineDiffContent::default(),
        },
        false,
    );
    let prompt = build_review_prompt(&batch(vec![comment]));
    // line_range is [4, 10) 0-indexed -> L5-L10 (1-indexed, both ends inclusive)
    assert!(prompt.contains("L5-L10"), "expected L5-L10, got: {prompt}",);
}

#[test]
fn test_build_review_prompt_file_level_comment() {
    let comment = make_comment(
        "needs refactoring",
        AttachedReviewCommentTarget::File {
            absolute_file_path: local_path("/repo/src/utils.rs"),
        },
        false,
    );
    let prompt = build_review_prompt(&batch(vec![comment]));
    assert!(prompt.contains("/repo/src/utils.rs: needs refactoring"));
    // Not a deleted file (empty diff_set), so no "deleted file" text.
    assert!(!prompt.contains("deleted file"));
}

#[test]
fn test_build_review_prompt_deleted_file_comment() {
    let comment = make_comment(
        "why remove this?",
        AttachedReviewCommentTarget::File {
            absolute_file_path: local_path("/repo/src/old.rs"),
        },
        false,
    );
    let mut review = batch(vec![comment]);
    review.diff_set.insert(
        "src/old.rs".to_string(),
        vec![DiffSetHunk {
            line_range: LineCount::from(0)..LineCount::from(5),
            diff_content: String::new(),
            lines_added: 0,
            lines_removed: 5,
        }],
    );
    let prompt = build_review_prompt(&review);
    assert!(
        prompt.contains("(deleted file"),
        "expected deleted file annotation, got: {prompt}",
    );
}

#[test]
fn test_build_review_prompt_general_comment() {
    let comment = make_comment(
        "overall looks good",
        AttachedReviewCommentTarget::General,
        false,
    );
    let prompt = build_review_prompt(&batch(vec![comment]));
    assert!(prompt.contains("General: overall looks good"));
}

#[test]
fn test_build_review_prompt_skips_outdated_comments() {
    let active = make_comment("keep me", AttachedReviewCommentTarget::General, false);
    let outdated = make_comment("skip me", AttachedReviewCommentTarget::General, true);
    let prompt = build_review_prompt(&batch(vec![active, outdated]));
    assert!(prompt.contains("keep me"));
    assert!(!prompt.contains("skip me"));
}

#[test]
fn test_build_review_prompt_multiple_comments() {
    let c1 = make_comment(
        "first",
        AttachedReviewCommentTarget::Line {
            absolute_file_path: local_path("/repo/a.rs"),
            line: EditorLineLocation::Current {
                line_number: LineCount::from(4),
                line_range: LineCount::from(4)..LineCount::from(5),
            },
            content: LineDiffContent::default(),
        },
        false,
    );
    let c2 = make_comment("second", AttachedReviewCommentTarget::General, false);
    let prompt = build_review_prompt(&batch(vec![c1, c2]));
    assert!(prompt.contains("/repo/a.rs L5: first"));
    assert!(prompt.contains("General: second"));
}

#[test]
fn test_build_review_prompt_exports_internal_markdown_without_punctuation_escapes() {
    let comment = make_comment("Fix this\\.", AttachedReviewCommentTarget::General, false);
    let prompt = build_review_prompt(&batch(vec![comment]));
    assert!(prompt.contains("General: Fix this."));
    assert!(!prompt.contains("Fix this\\."));
}

// ---------------------------------------------------------------------------
// build_diff_hunk_prompt tests
// ---------------------------------------------------------------------------

#[test]
fn test_build_diff_hunk_prompt_format() {
    let prompt = build_diff_hunk_prompt("/repo/src/main.rs", 10, 20, 3, 2);
    assert_eq!(
        prompt,
        "/repo/src/main.rs L10-L20 (+3 -2) -- run `git diff` to see the full context.",
    );
}

// ---------------------------------------------------------------------------
// build_selection_line_range_prompt tests
// ---------------------------------------------------------------------------

#[test]
fn test_build_selection_line_range_prompt_format() {
    let result = build_selection_line_range_prompt("src/foo.rs", 5, 10);
    assert_eq!(result, "src/foo.rs L5-L10");
}

#[test]
fn test_build_selection_substring_prompt_format() {
    let result = build_selection_substring_prompt("src/foo.rs", 5, "let x = 42;");
    assert_eq!(result, "src/foo.rs L5: let x = 42;");
}

#[test]
fn test_detect_known_agents() {
    for (command, expected) in [
        ("claude", CLIAgent::Claude),
        ("gemini", CLIAgent::Gemini),
        ("codex", CLIAgent::Codex),
        ("amp", CLIAgent::Amp),
        ("droid", CLIAgent::Droid),
        ("opencode", CLIAgent::OpenCode),
        ("copilot", CLIAgent::Copilot),
        ("agent", CLIAgent::CursorCli),
        ("goose", CLIAgent::Goose),
        ("vibe", CLIAgent::Vibe),
        ("agy", CLIAgent::Antigravity),
        ("omp", CLIAgent::OhMyPi),
        ("grok", CLIAgent::Grok),
        ("muse", CLIAgent::Muse),
    ] {
        assert_eq!(
            CLIAgent::detect(command, None, None),
            Some(expected),
            "failed to detect {command}",
        );
    }
}

#[test]
fn test_detect_with_arguments() {
    assert_eq!(
        CLIAgent::detect("claude --model opus", None, None),
        Some(CLIAgent::Claude),
    );
    assert_eq!(
        CLIAgent::detect("gemini chat", None, None),
        Some(CLIAgent::Gemini),
    );
    assert_eq!(
        CLIAgent::detect(r#"muse exec "ping""#, None, None),
        Some(CLIAgent::Muse),
    );
    assert_eq!(CLIAgent::detect("npx muse", None, None), None);
}

#[test]
fn test_grok_public_configuration() {
    assert_eq!(CLIAgent::Grok.command_prefix(), "grok");
    assert_eq!(CLIAgent::Grok.display_name(), "Grok Build");
    assert!(CLIAgent::Grok.supports_bash_mode());
    assert_eq!(CLIAgent::Grok.skill_command_prefix(), "/");
    assert_eq!(CLIAgent::Grok.icon(), Some(Icon::GrokLogo));
}

#[test]
fn test_muse_public_configuration() {
    assert_eq!(CLIAgent::Muse.command_prefix(), "muse");
    assert_eq!(CLIAgent::Muse.display_name(), "Muse Code");
    assert!(CLIAgent::Muse.supports_bash_mode());
    assert_eq!(CLIAgent::Muse.skill_command_prefix(), "/");
    assert_eq!(CLIAgent::Muse.icon(), Some(Icon::MuseLogo));
}

#[test]
fn test_detect_vibe_acp_binary() {
    // The mistral-vibe package ships a `vibe-acp` ACP-mode binary alongside
    // the user-facing `vibe` TUI. Both must be detected as the same agent.
    assert_eq!(
        CLIAgent::detect("vibe-acp", None, None),
        Some(CLIAgent::Vibe),
    );
    assert_eq!(
        CLIAgent::detect("vibe-acp --some-flag", None, None),
        Some(CLIAgent::Vibe),
    );
    // Distinct binary names should not bleed into Vibe.
    assert_eq!(CLIAgent::detect("vibe-other", None, None), None);
}

#[test]
fn test_detect_with_leading_whitespace() {
    assert_eq!(
        CLIAgent::detect("  claude", None, None),
        Some(CLIAgent::Claude),
    );
    assert_eq!(
        CLIAgent::detect("\tclaude --help", None, None),
        Some(CLIAgent::Claude),
    );
}

#[test]
fn test_detect_no_match() {
    assert_eq!(CLIAgent::detect("ls -la", None, None), None);
    assert_eq!(CLIAgent::detect("vim", None, None), None);
    assert_eq!(CLIAgent::detect("claude_wrapper", None, None), None);
}

#[test]
fn test_detect_with_alias() {
    let map = aliases(&[("c", "claude")]);
    assert_eq!(
        CLIAgent::detect("c", None, Some(&map)),
        Some(CLIAgent::Claude),
    );
    assert_eq!(
        CLIAgent::detect("c --help", None, Some(&map)),
        Some(CLIAgent::Claude),
    );

    let map = aliases(&[("o", "omp")]);
    assert_eq!(
        CLIAgent::detect("o", None, Some(&map)),
        Some(CLIAgent::OhMyPi),
    );
}

#[test]
fn test_detect_alias_not_matching() {
    let map = aliases(&[("c", "cat")]);
    assert_eq!(CLIAgent::detect("c", None, Some(&map)), None);
}

#[test]
fn test_detect_alias_multi_word_value() {
    // Alias whose value starts with "gemini" but has extra words
    let map = aliases(&[("g", "gemini chat --verbose")]);
    assert_eq!(
        CLIAgent::detect("g", None, Some(&map)),
        Some(CLIAgent::Gemini),
    );
}

#[test]
fn test_detect_with_env_var_prefix() {
    assert_eq!(
        CLIAgent::detect("EXAMPLE=true opencode", Some(EscapeChar::Backslash), None),
        Some(CLIAgent::OpenCode),
    );
    assert_eq!(
        CLIAgent::detect("FOO=1 omp", Some(EscapeChar::Backslash), None),
        Some(CLIAgent::OhMyPi),
    );
}

#[test]
fn test_detect_with_multiple_env_vars() {
    assert_eq!(
        CLIAgent::detect(
            "FOO=1 BAR=2 opencode --flag",
            Some(EscapeChar::Backslash),
            None
        ),
        Some(CLIAgent::OpenCode),
    );
}

#[test]
fn test_detect_with_alias_and_env_var() {
    let map = aliases(&[("oc", "EXAMPLE=1 opencode")]);
    assert_eq!(
        CLIAgent::detect("oc --flag", Some(EscapeChar::Backslash), Some(&map)),
        Some(CLIAgent::OpenCode),
    );
}

#[test]
fn test_serialized_name_round_trips_known_agents() {
    for agent in enum_iterator::all::<CLIAgent>() {
        let name = agent.to_serialized_name();
        if agent == CLIAgent::Unknown {
            assert_eq!(name, "Unknown");
        } else {
            assert!(!name.is_empty(), "empty serialized name for {agent:?}");
        }
        assert_eq!(
            CLIAgent::from_serialized_name(&name),
            agent,
            "round-trip failed for {agent:?} with serialized name {name:?}",
        );
    }
}

#[test]
fn test_from_serialized_name_falls_back_to_unknown() {
    assert_eq!(CLIAgent::from_serialized_name(""), CLIAgent::Unknown);
    assert_eq!(
        CLIAgent::from_serialized_name("nonexistent"),
        CLIAgent::Unknown
    );
}

#[test]
fn test_oh_my_pi_supports_bash_mode() {
    assert!(CLIAgent::OhMyPi.supports_bash_mode());
}
