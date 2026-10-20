use super::*;

#[test]
fn projects_start_empty() {
    assert!(ProjectDirectories::default_value().is_empty());
    assert!(ProjectParentDirectories::default_value().is_empty());
}

#[test]
fn discovery_is_one_level_deduplicated_and_reports_unavailable_locations() {
    let root = tempfile::tempdir().unwrap();
    let first = root.path().join("one");
    let nested = first.join("nested");
    let second = root.path().join("two");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::create_dir(&second).unwrap();
    std::fs::write(root.path().join("not-a-folder"), "").unwrap();
    let missing = root.path().join("missing");
    let (projects, unavailable) = discover(
        &[first.clone(), first.clone(), missing.clone()],
        &[root.path().to_owned()],
    );
    assert_eq!(projects.len(), 2);
    assert!(
        projects
            .iter()
            .any(|p| p.path == first.canonicalize().unwrap())
    );
    assert!(
        !projects
            .iter()
            .any(|p| p.path == nested.canonicalize().unwrap())
    );
    assert_eq!(unavailable, vec![missing]);
}

#[test]
fn nested_projects_win_and_similar_prefixes_stay_separate() {
    let root = tempfile::tempdir().unwrap();
    let outer = root.path().join("app");
    let inner = outer.join("client");
    let projects = vec![
        LocalProject {
            path: outer.clone(),
            name: "app".into(),
        },
        LocalProject {
            path: inner.clone(),
            name: "client".into(),
        },
    ];
    assert_eq!(
        project_for_directory(&projects, &inner.join("src")),
        Some(&projects[1])
    );
    assert_eq!(
        project_for_directory(&projects, &outer.join("server")),
        Some(&projects[0])
    );
    assert!(project_for_directory(&projects, &root.path().join("app-extra")).is_none());
}

#[test]
fn duplicate_basenames_are_distinct_projects() {
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("one/app");
    let b = root.path().join("two/app");
    std::fs::create_dir_all(&a).unwrap();
    std::fs::create_dir_all(&b).unwrap();
    let (projects, _) = discover(&[a, b], &[]);
    assert_eq!(projects.len(), 2);
    assert_eq!(projects[0].name, projects[1].name);
    assert_ne!(projects[0].path, projects[1].path);
}

#[cfg(unix)]
#[test]
fn symlink_aliases_do_not_duplicate_projects() {
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let alias = root.path().join("alias");
    std::fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, &alias).unwrap();
    assert_eq!(discover(&[real, alias], &[]).0.len(), 1);
}
