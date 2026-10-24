use super::*;

#[test]
fn compose_patterns_includes_defaults_when_user_and_enterprise_are_empty() {
    let patterns = compose_patterns(std::iter::empty(), std::iter::empty());
    assert_eq!(patterns.len(), DEFAULT_REGEXES_WITH_NAMES.len());
    for default in DEFAULT_REGEXES_WITH_NAMES {
        assert!(
            patterns.contains(&default.pattern),
            "expected default pattern {} to be present",
            default.pattern,
        );
    }
}

#[test]
fn compose_patterns_layers_user_and_enterprise_on_top_of_defaults() {
    let user = [r"\bUSER-\d+\b"];
    let enterprise = [r"\bENT-\d+\b"];
    let patterns = compose_patterns(user.iter().copied(), enterprise.iter().copied());
    // Enterprise comes first, then user, then defaults.
    assert_eq!(patterns[0], r"\bENT-\d+\b");
    assert_eq!(patterns[1], r"\bUSER-\d+\b");
    // Defaults are still all present.
    for default in DEFAULT_REGEXES_WITH_NAMES {
        assert!(
            patterns.contains(&default.pattern),
            "expected default pattern {} to be present alongside user/enterprise",
            default.pattern,
        );
    }
}

#[test]
fn compose_patterns_dedups_user_pattern_that_matches_a_default() {
    // Pick the first default pattern; passing the same string as a "user"
    // pattern should not cause it to appear twice in the composed list.
    let duplicated = DEFAULT_REGEXES_WITH_NAMES[0].pattern;
    let patterns = compose_patterns(std::iter::once(duplicated), std::iter::empty());
    let occurrences = patterns.iter().filter(|p| **p == duplicated).count();
    assert_eq!(
        occurrences, 1,
        "duplicate pattern should appear at most once in composed list",
    );
    // Total length is the defaults (the user pattern was deduped away).
    assert_eq!(patterns.len(), DEFAULT_REGEXES_WITH_NAMES.len());
}

#[test]
fn compose_patterns_dedups_enterprise_pattern_that_matches_a_user_pattern() {
    let user = [r"\bSHARED-\d+\b"];
    let enterprise = [r"\bSHARED-\d+\b"];
    let patterns = compose_patterns(user.iter().copied(), enterprise.iter().copied());
    let occurrences = patterns.iter().filter(|p| **p == r"\bSHARED-\d+\b").count();
    assert_eq!(occurrences, 1);
}
