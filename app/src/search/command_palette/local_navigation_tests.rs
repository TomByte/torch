use super::*;
use warpui::{EntityId, WindowId};

fn source(path: &std::path::Path) -> DataSource {
    let mut source = DataSource::default();
    source.set_tabs(
        vec!["landing-hero", "pricing-page"]
            .into_iter()
            .enumerate()
            .map(|(index, title)| TabNavigationData {
                pane_group_id: EntityId::from_usize(index),
                title: title.into(),
                subtitle: Some(path.to_string_lossy().into_owned()),
                window_id: WindowId::from_usize(1),
                tab_index: index + 1,
                color: None,
            })
            .collect(),
    );
    source
}

fn query(text: &str) -> Query {
    Query {
        text: text.into(),
        filters: HashSet::new(),
    }
}

#[test]
fn empty_query_prioritizes_sessions_and_offers_creation_without_duplicate_directories() {
    let directory = tempfile::tempdir().unwrap();
    let results = source(directory.path()).search(&query(""));
    assert_eq!(results.len(), 6);
    assert!(
        matches!(results[0].accept_result(), CommandPaletteItemAction::NavigateToTab { pane_group_id, .. } if pane_group_id == EntityId::from_usize(0))
    );
    assert!(results[0].priority_tier() > results[2].priority_tier());
    assert!(results[2].priority_tier() > results[4].priority_tier());
    assert!(matches!(
        results[2].accept_result(),
        CommandPaletteItemAction::NewLocalSession { directory: None }
    ));
    assert!(matches!(
        results[3].accept_result(),
        CommandPaletteItemAction::ChooseLocalDirectory
    ));
}

#[test]
fn fuzzy_session_title_and_filters_select_the_correct_action() {
    let directory = tempfile::tempdir().unwrap();
    let source = source(directory.path());
    let results = source.search(&query("pricpage"));
    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0].accept_result(), CommandPaletteItemAction::NavigateToTab { pane_group_id, .. } if pane_group_id == EntityId::from_usize(1))
    );
    let mut filtered = query("");
    filtered.filters.insert(QueryFilter::Sessions);
    assert_eq!(source.search(&filtered).len(), 2);
}

#[test]
fn typed_directory_preserves_spaces_and_never_becomes_a_shell_command() {
    let parent = tempfile::tempdir().unwrap();
    let path = parent.path().join("folder with spaces; echo example");
    std::fs::create_dir(&path).unwrap();
    let results = DataSource::default().search(&query(path.to_str().unwrap()));
    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0].accept_result(), CommandPaletteItemAction::NewLocalSession { directory: Some(ref directory) } if directory == &path)
    );
    assert!(directory_path("relative/path").is_none());
    assert!(directory_path(parent.path().join("missing").to_str().unwrap()).is_none());
    assert_eq!(directory_path("~"), dirs::home_dir());
}

#[test]
fn exact_creation_and_directory_requests_rank_above_fuzzy_matches() {
    let directory = tempfile::tempdir().unwrap();
    let mut source = source(directory.path());
    source.tabs[0].title = "New session".into();
    let best = |text: &str| {
        source
            .search(&query(text))
            .into_iter()
            .max_by_key(|item| (item.priority_tier(), item.score()))
            .unwrap()
            .accept_result()
    };
    assert!(matches!(
        best("new session"),
        CommandPaletteItemAction::NewLocalSession { directory: None }
    ));
    assert!(
        matches!(best(directory.path().to_str().unwrap()), CommandPaletteItemAction::NewLocalSession { directory: Some(ref path) } if path == directory.path())
    );
}

#[test]
fn project_selection_resumes_a_session_in_a_subfolder_and_offers_another() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().canonicalize().unwrap();
    let mut source = source(&path.join("src"));
    source.projects = vec![LocalProject {
        path: path.clone(),
        name: "landing".into(),
    }];
    let results = source.search(&query("landing"));
    let best = results
        .iter()
        .max_by_key(|item| (item.priority_tier(), item.score()))
        .unwrap();
    assert!(
        matches!(best.accept_result(), CommandPaletteItemAction::NavigateToTab { pane_group_id, .. } if pane_group_id == EntityId::from_usize(0))
    );
    assert!(results.iter().any(|item| matches!(item.accept_result(), CommandPaletteItemAction::NewLocalSession { directory: Some(ref directory) } if directory == &path)));
    source.tabs.clear();
    let best = source
        .search(&query("landing"))
        .into_iter()
        .max_by_key(|item| (item.priority_tier(), item.score()))
        .unwrap();
    assert!(
        matches!(best.accept_result(), CommandPaletteItemAction::NewLocalSession { directory: Some(ref directory) } if directory == &path)
    );
}

#[cfg(unix)]
#[test]
fn project_selection_resumes_alias_directory_without_changing_display_path() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&project, &alias).unwrap();
    let mut source = source(&alias.join("src"));
    source.set_projects(vec![LocalProject {
        path: project.canonicalize().unwrap(),
        name: "project".into(),
    }]);
    let best = source
        .search(&query("project"))
        .into_iter()
        .max_by_key(|item| (item.priority_tier(), item.score()))
        .unwrap();
    assert!(
        matches!(best.accept_result(), CommandPaletteItemAction::NavigateToTab { pane_group_id, .. } if pane_group_id == EntityId::from_usize(0))
    );
    assert_eq!(
        source.tabs[0].subtitle.as_deref(),
        alias.join("src").to_str()
    );
    assert!(
        project_for_directory(
            &source.projects,
            &source.session_directories[&EntityId::from_usize(0)]
        )
        .is_some()
    );
}
