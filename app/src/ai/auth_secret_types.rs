use torch_cli::agent::Harness;

/// Whether managed auth secrets can authenticate runs of `harness`.
pub fn harness_supports_auth_secrets(harness: Harness) -> bool {
    match harness {
        Harness::Claude | Harness::Codex => true,
        Harness::Oz | Harness::OpenCode | Harness::Gemini | Harness::Pi | Harness::Unknown => false,
    }
}
