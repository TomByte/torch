use super::*;

#[test]
fn claude_and_codex_sessions_have_resume_commands() {
    let claude =
        ResumableAgentSession::new(CLIAgent::Claude, "5f0c2a1e-9b7d-4c3e-8a6f-1d2e3f4a5b6c")
            .unwrap();
    assert_eq!(
        claude.resume_command(),
        "claude --resume 5f0c2a1e-9b7d-4c3e-8a6f-1d2e3f4a5b6c"
    );
    let codex = ResumableAgentSession::new(CLIAgent::Codex, "thread_01").unwrap();
    assert_eq!(codex.resume_command(), "codex resume thread_01");
}

#[test]
fn other_agents_and_unsafe_ids_are_not_resumable() {
    assert!(ResumableAgentSession::new(CLIAgent::Gemini, "abc").is_none());
    for id in ["", "abc; rm -rf ~", "$(whoami)", "a b", &"x".repeat(129)] {
        assert!(
            ResumableAgentSession::new(CLIAgent::Claude, id).is_none(),
            "{id}"
        );
    }
}

#[test]
fn json_round_trips_and_rejects_tampered_ids() {
    let session = ResumableAgentSession::new(CLIAgent::Codex, "thread-1").unwrap();
    let json = session.to_json().unwrap();
    assert_eq!(ResumableAgentSession::from_json(&json), Some(session));
    assert!(ResumableAgentSession::from_json(r#"{"agent":"claude","session_id":"x;y"}"#).is_none());
    assert!(ResumableAgentSession::from_json(r#"{"agent":"gemini","session_id":"x"}"#).is_none());
    assert!(ResumableAgentSession::from_json("not json").is_none());
}
