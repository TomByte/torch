use std::cmp::Ordering;

use super::{compare_versions, plugin_manager};

#[test]
fn never_offers_a_plugin_manager() {
    assert!(plugin_manager().is_none());
}

#[test]
fn compare_versions_equal() {
    assert_eq!(compare_versions("1.2.3", "1.2.3"), Ordering::Equal);
}

#[test]
fn compare_versions_less_than_major() {
    assert_eq!(compare_versions("1.0.0", "2.0.0"), Ordering::Less);
}

#[test]
fn compare_versions_less_than_minor() {
    assert_eq!(compare_versions("1.1.0", "1.2.0"), Ordering::Less);
}

#[test]
fn compare_versions_less_than_patch() {
    assert_eq!(compare_versions("1.1.0", "1.1.1"), Ordering::Less);
}

#[test]
fn compare_versions_greater_than() {
    assert_eq!(compare_versions("3.0.0", "2.0.0"), Ordering::Greater);
}

#[test]
fn compare_versions_unparseable_treated_as_zero() {
    assert_eq!(compare_versions("abc", "0.0.0"), Ordering::Equal);
    assert_eq!(compare_versions("abc", "1.0.0"), Ordering::Less);
}

#[test]
fn compare_versions_partial_version_string() {
    assert_eq!(compare_versions("2", "2.0.0"), Ordering::Equal);
    assert_eq!(compare_versions("2.1", "2.1.0"), Ordering::Equal);
}

#[test]
fn compare_versions_empty_string() {
    assert_eq!(compare_versions("", "2.0.0"), Ordering::Less);
}
