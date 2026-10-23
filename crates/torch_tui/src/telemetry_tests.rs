use torch_core::telemetry::TelemetryEvent as _;

use super::*;

#[test]
fn startup_payload_reports_sanitized_terminal_metadata() {
    let event = TuiStartupTelemetryEvent {
        term_program: sanitize_term_program(Some("  ghost\u{1b}ty  ".into())),
        multiplexer: TuiHostMultiplexer::Tmux,
    };

    assert_eq!(
        event.payload(),
        Some(json!({
            "term_program": "ghostty",
            "multiplexer": "tmux",
        }))
    );
}

#[test]
fn terminal_program_is_bounded_and_empty_values_are_omitted() {
    assert_eq!(sanitize_term_program(Some("   ".into())), None);
    assert_eq!(
        sanitize_term_program(Some("x".repeat(MAX_TERM_PROGRAM_CHARS + 10).into()))
            .unwrap()
            .len(),
        MAX_TERM_PROGRAM_CHARS
    );
}

#[test]
fn multiplexer_detection_uses_stable_precedence() {
    let value = std::ffi::OsStr::new("present");
    assert_eq!(
        detect_multiplexer(Some(value), Some(value), Some(value), Some(value)).as_str(),
        "tmux"
    );
    assert_eq!(
        detect_multiplexer(None, Some(value), Some(value), Some(value)).as_str(),
        "screen"
    );
    assert_eq!(
        detect_multiplexer(None, None, Some(value), None).as_str(),
        "zellij"
    );
    assert_eq!(detect_multiplexer(None, None, None, None).as_str(), "none");
}
