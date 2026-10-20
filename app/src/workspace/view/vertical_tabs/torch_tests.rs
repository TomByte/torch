use super::*;

#[test]
fn ranges_follow_folder_order_instead_of_underlying_tab_indices() {
    let keys = [
        "folder:A",
        "folder:B",
        "folder:A",
        "manual:review",
        "folder:B",
    ]
    .map(str::to_owned);
    let groups = grouped_indices(&keys);
    assert_eq!(groups, vec![vec![0, 2], vec![1, 4], vec![3]]);
    let order = groups.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(range_in_order(&order, 0, 2), vec![0, 2]);
    assert_eq!(range_in_order(&order, 3, 1), vec![1, 4, 3]);
    assert!(range_in_order(&order, 99, 1).is_empty());
}

#[test]
fn folder_groups_handle_home_roots_and_both_path_separators() {
    assert_eq!(folder_label("~/code/launchpad/"), "launchpad");
    assert_eq!(folder_label(r"C:\code\backend\"), "backend");
    assert_eq!(folder_label("~"), "Other");
    assert_eq!(folder_label("/"), "Other");
    assert_eq!(folder_label(""), "Other");
}

#[test]
fn grouping_merges_home_and_nonterminal_sessions_but_not_equal_folder_names() {
    assert_eq!(folder_key("~"), folder_key(""));
    assert_eq!(folder_key("~/code/torch/"), folder_key("~/code/torch"));
    assert_ne!(folder_key("~/work/torch"), folder_key("~/personal/torch"));
}

#[test]
fn activity_labels_remain_compact_across_time_boundaries() {
    for (minutes, expected) in [
        (-1, "now"),
        (0, "now"),
        (1, "1m"),
        (59, "59m"),
        (60, "1h"),
        (1439, "23h"),
        (1440, "1d"),
    ] {
        assert_eq!(age_label(minutes), expected);
    }
}
