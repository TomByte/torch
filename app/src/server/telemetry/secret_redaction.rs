//! Best-effort secret redaction for telemetry payloads.
//!
//! Unlike the AI-side secret redaction in `app/src/ai/blocklist/block/secret_redaction.rs`,
//! which is gated on the user's secret-redaction (a.k.a. "safe mode") setting and is used
//! for visual obfuscation in the terminal, the redaction in this module is unconditional:
//! we always do a redaction pass on telemetry payloads that may contain user-generated
//! content, regardless of the user's safe-mode setting. The two settings are deliberately
//! decoupled — visual obfuscation is a UX preference, while telemetry-side redaction is a
//! defence-in-depth measure for data leaving the device.
//!
//! The regex used for redaction always includes the default patterns defined in
//! `crate::terminal::model::secrets::regexes::DEFAULT_REGEXES_WITH_NAMES`. Any custom
//! patterns the user has configured (or that their organization has configured via
//! enterprise secret redaction) are layered on top of those defaults.
//!
//! This module is intentionally lightweight: it does byte-range matching only and does
//! not track `SecretLevel`s or character ranges, since the telemetry path doesn't need
//! either.
use std::collections::HashSet;

use lazy_static::lazy_static;
use parking_lot::RwLock;
use regex_automata::meta::Regex;
use torch_errors::report_error;

use crate::terminal::model::secrets::regexes::DEFAULT_REGEXES_WITH_NAMES;
lazy_static! {
    /// Regex used to redact secrets from telemetry payloads. Initialized with the
    /// default patterns so that redaction works even before the user's privacy
    /// settings are loaded (and even for users who have never configured any
    /// custom patterns).
    static ref TELEMETRY_SECRETS_REGEX: RwLock<Regex> = RwLock::new(build_default_regex());
}
/// Builds a regex containing only the default patterns. Used to seed the static
/// regex before the privacy settings are loaded.
fn build_default_regex() -> Regex {
    let patterns: Vec<&str> = DEFAULT_REGEXES_WITH_NAMES
        .iter()
        .map(|d| d.pattern)
        .collect();
    Regex::new_many(&patterns).expect("default secret patterns should compile")
}
/// Rebuilds [`TELEMETRY_SECRETS_REGEX`] from the user's and enterprise's secret
/// regex lists, layered on top of the default patterns. The default patterns are
/// always included, so redaction works even when the user has not configured any
/// custom patterns.
pub fn update_telemetry_secrets_regex<'a, U, E>(user_secrets: U, enterprise_secrets: E)
where
    U: IntoIterator<Item = &'a regex::Regex>,
    E: IntoIterator<Item = &'a regex::Regex>,
{
    let patterns = compose_patterns(
        user_secrets.into_iter().map(regex::Regex::as_str),
        enterprise_secrets.into_iter().map(regex::Regex::as_str),
    );
    match Regex::new_many(&patterns) {
        Ok(regex) => *TELEMETRY_SECRETS_REGEX.write() = regex,
        Err(err) => {
            report_error!(
                anyhow::Error::new(err).context("Failed to build telemetry secrets regex")
            )
        }
    }
}
/// Composes the full list of patterns to compile into the telemetry regex,
/// ordered enterprise → user → defaults, with later occurrences of an already-
/// seen pattern string deduped out.
fn compose_patterns<'a>(
    user: impl Iterator<Item = &'a str>,
    enterprise: impl Iterator<Item = &'a str>,
) -> Vec<&'a str> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut patterns: Vec<&str> = Vec::new();
    let all = enterprise
        .chain(user)
        .chain(DEFAULT_REGEXES_WITH_NAMES.iter().map(|d| d.pattern));
    for pattern in all {
        if seen.insert(pattern) {
            patterns.push(pattern);
        }
    }
    patterns
}
#[cfg(test)]
#[path = "secret_redaction_tests.rs"]
mod tests;
