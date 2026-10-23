use super::{Availability, commands};

#[test]
fn argument_hint_uses_shared_command_prefix_and_text() {
    let hint = commands::EXPORT_TO_FILE
        .argument_hint()
        .expect("export-to-file has an argument hint");

    assert_eq!(hint.input_prefix, "/export-to-file ");
    assert_eq!(hint.text, "<optional filename>");
    assert_eq!(commands::EXPORT_TO_CLIPBOARD.argument_hint(), None);
}

/// Helper: constructs a session context for an agent view in a local session with a repo,
/// no active LRC, and an active conversation.
fn local_agent_view_with_repo() -> Availability {
    Availability::AGENT_VIEW
        | Availability::LOCAL
        | Availability::REPOSITORY
        | Availability::NO_LRC_CONTROL
        | Availability::ACTIVE_CONVERSATION
}

/// Helper: constructs a session context for a terminal view in a local session with a repo,
/// no active LRC, and an active conversation.
fn local_terminal_view_with_repo() -> Availability {
    Availability::TERMINAL_VIEW
        | Availability::LOCAL
        | Availability::REPOSITORY
        | Availability::NO_LRC_CONTROL
        | Availability::ACTIVE_CONVERSATION
}

// --- ALWAYS ---

#[test]
fn always_available_in_any_context() {
    let command = Availability::ALWAYS;
    assert!(local_agent_view_with_repo().contains(command));
    assert!(local_terminal_view_with_repo().contains(command));
    // Even a minimal remote session satisfies ALWAYS.
    assert!(Availability::AGENT_VIEW.contains(command));
}

// --- View flag tests ---

#[test]
fn no_view_requirement_available_in_any_view() {
    let command = Availability::ALWAYS;
    assert!(local_agent_view_with_repo().contains(command));
    assert!(local_terminal_view_with_repo().contains(command));
}

#[test]
fn agent_view_requirement_only_in_agent_view() {
    let command = Availability::AGENT_VIEW;
    assert!(local_agent_view_with_repo().contains(command));
    assert!(!local_terminal_view_with_repo().contains(command));
}

#[test]
fn terminal_view_requirement_only_in_terminal_view() {
    let command = Availability::TERMINAL_VIEW;
    assert!(!local_agent_view_with_repo().contains(command));
    assert!(local_terminal_view_with_repo().contains(command));
}

#[test]
fn both_view_bits_satisfy_either_view_requirement() {
    // When AgentView feature flag is disabled, both view bits are set.
    let session = Availability::AGENT_VIEW | Availability::TERMINAL_VIEW | Availability::LOCAL;
    assert!(session.contains(Availability::AGENT_VIEW));
    assert!(session.contains(Availability::TERMINAL_VIEW));
    assert!(session.contains(Availability::ALWAYS));
}

// --- Repository flag tests ---

#[test]
fn repository_requirement_satisfied_when_in_repo() {
    let command = Availability::REPOSITORY;
    let session = Availability::AGENT_VIEW | Availability::LOCAL | Availability::REPOSITORY;
    assert!(session.contains(command));
}

#[test]
fn repository_requirement_not_satisfied_when_not_in_repo() {
    let command = Availability::REPOSITORY;
    let session = Availability::AGENT_VIEW | Availability::LOCAL;
    assert!(!session.contains(command));
}

#[test]
fn no_repository_requirement_available_regardless() {
    let command = Availability::ALWAYS;
    let session_with_repo =
        Availability::AGENT_VIEW | Availability::LOCAL | Availability::REPOSITORY;
    let session_without_repo = Availability::AGENT_VIEW | Availability::LOCAL;
    assert!(session_with_repo.contains(command));
    assert!(session_without_repo.contains(command));
}

// --- LOCAL flag tests ---

#[test]
fn local_requirement_satisfied_in_local_session() {
    let command = Availability::LOCAL;
    let session = Availability::AGENT_VIEW | Availability::LOCAL;
    assert!(session.contains(command));
}

#[test]
fn local_requirement_not_satisfied_in_remote_session() {
    let command = Availability::LOCAL;
    let session = Availability::AGENT_VIEW; // remote: no LOCAL flag
    assert!(!session.contains(command));
}

#[test]
fn no_local_requirement_available_in_any_session_type() {
    let command = Availability::ALWAYS;
    let local_session = Availability::AGENT_VIEW | Availability::LOCAL;
    let remote_session = Availability::AGENT_VIEW;
    assert!(local_session.contains(command));
    assert!(remote_session.contains(command));
}

// --- NO_LRC_CONTROL flag tests ---

#[test]
fn no_lrc_control_requirement_satisfied_when_not_in_control() {
    let command = Availability::NO_LRC_CONTROL;
    let session = Availability::AGENT_VIEW | Availability::NO_LRC_CONTROL;
    assert!(session.contains(command));
}

#[test]
fn no_lrc_control_requirement_not_satisfied_when_in_control() {
    let command = Availability::NO_LRC_CONTROL;
    let session = Availability::AGENT_VIEW; // agent is in control: no NO_LRC_CONTROL flag
    assert!(!session.contains(command));
}

// --- ACTIVE_CONVERSATION flag tests ---

// --- CODEBASE_CONTEXT flag tests ---

// --- CLOUD_AGENT flag tests ---

// --- CLOUD_MODE_V2_COMPOSER flag tests ---

// --- AI_ENABLED flag tests ---

// --- Combined flag tests ---

#[test]
fn agent_view_and_repository_both_required() {
    let command = Availability::AGENT_VIEW | Availability::REPOSITORY;

    // Agent view + repo → available
    let session = Availability::AGENT_VIEW | Availability::LOCAL | Availability::REPOSITORY;
    assert!(session.contains(command));

    // Terminal view + repo → not available (missing AGENT_VIEW)
    let session = Availability::TERMINAL_VIEW | Availability::LOCAL | Availability::REPOSITORY;
    assert!(!session.contains(command));

    // Agent view, no repo → not available (missing REPOSITORY)
    let session = Availability::AGENT_VIEW | Availability::LOCAL;
    assert!(!session.contains(command));
}

#[test]
fn agent_view_and_local_both_required() {
    let command = Availability::AGENT_VIEW | Availability::LOCAL;

    // Agent view + local → available
    let session = Availability::AGENT_VIEW | Availability::LOCAL;
    assert!(session.contains(command));

    // Agent view + remote → not available (missing LOCAL)
    let session = Availability::AGENT_VIEW;
    assert!(!session.contains(command));

    // Terminal view + local → not available (missing AGENT_VIEW)
    let session = Availability::TERMINAL_VIEW | Availability::LOCAL;
    assert!(!session.contains(command));
}

#[test]
fn fork_like_command_requires_agent_view_active_conversation_no_lrc() {
    let command =
        Availability::AGENT_VIEW | Availability::ACTIVE_CONVERSATION | Availability::NO_LRC_CONTROL;

    // All conditions met → available
    let session = Availability::AGENT_VIEW
        | Availability::ACTIVE_CONVERSATION
        | Availability::NO_LRC_CONTROL
        | Availability::LOCAL;
    assert!(session.contains(command));

    // Agent in control of LRC → not available
    let session = Availability::AGENT_VIEW | Availability::ACTIVE_CONVERSATION;
    assert!(!session.contains(command));

    // No active conversation → not available
    let session = Availability::AGENT_VIEW | Availability::NO_LRC_CONTROL;
    assert!(!session.contains(command));

    // Wrong view → not available
    let session = Availability::TERMINAL_VIEW
        | Availability::ACTIVE_CONVERSATION
        | Availability::NO_LRC_CONTROL;
    assert!(!session.contains(command));
}

#[test]
fn all_flags_required() {
    let command = Availability::AGENT_VIEW
        | Availability::LOCAL
        | Availability::REPOSITORY
        | Availability::NO_LRC_CONTROL
        | Availability::ACTIVE_CONVERSATION;

    let full_session = local_agent_view_with_repo();
    assert!(full_session.contains(command));

    // Missing any single flag → not available
    let missing_local = Availability::AGENT_VIEW
        | Availability::REPOSITORY
        | Availability::NO_LRC_CONTROL
        | Availability::ACTIVE_CONVERSATION;
    assert!(!missing_local.contains(command));

    let missing_repo = Availability::AGENT_VIEW
        | Availability::LOCAL
        | Availability::NO_LRC_CONTROL
        | Availability::ACTIVE_CONVERSATION;
    assert!(!missing_repo.contains(command));

    let missing_view = Availability::LOCAL
        | Availability::REPOSITORY
        | Availability::NO_LRC_CONTROL
        | Availability::ACTIVE_CONVERSATION;
    assert!(!missing_view.contains(command));
}
