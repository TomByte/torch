use super::*;

#[test]
fn time_in_state_stays_compact() {
    for (seconds, label) in [
        (0, "just now"),
        (59, "just now"),
        (60, "1m"),
        (59 * 60, "59m"),
        (60 * 60, "1h"),
        (25 * 60 * 60, "1d"),
    ] {
        assert_eq!(elapsed_label(Duration::from_secs(seconds)), label);
    }
}
