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

fn project(path: &Path) -> LocalProject {
    LocalProject {
        path: path.canonicalize().unwrap(),
        name: "repo".to_owned(),
    }
}

#[test]
fn linked_worktrees_belong_to_their_repository_project() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let git_dir = repo.join(".git").join("worktrees").join("feature");
    std::fs::create_dir_all(&git_dir).unwrap();
    std::fs::write(git_dir.join("commondir"), "../..\n").unwrap();
    let worktree = root.path().join("elsewhere").join("feature");
    std::fs::create_dir_all(worktree.join("src")).unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", git_dir.display()),
    )
    .unwrap();
    let projects = vec![project(&repo)];
    assert_eq!(
        project_for_directory(&projects, &worktree.join("src")),
        Some(&projects[0])
    );
}

#[test]
fn submodules_and_unrelated_folders_stay_outside_projects() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    let module_dir = repo.join(".git").join("modules").join("sub");
    std::fs::create_dir_all(&module_dir).unwrap();
    let submodule = root.path().join("other").join("sub");
    std::fs::create_dir_all(&submodule).unwrap();
    std::fs::write(
        submodule.join(".git"),
        format!("gitdir: {}\n", module_dir.display()),
    )
    .unwrap();
    let unrelated = root.path().join("plain");
    std::fs::create_dir_all(&unrelated).unwrap();
    let projects = vec![project(&repo)];
    assert!(project_for_directory(&projects, &submodule).is_none());
    assert!(project_for_directory(&projects, &unrelated).is_none());
}

#[test]
fn real_linked_worktrees_resolve_to_the_main_repository() {
    use std::process::Command;
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    let run = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
    };
    let Ok(init) = run(&["init", "-q"]) else {
        return;
    };
    assert!(init.status.success());
    let commit = run(&[
        "-c",
        "user.name=Torch",
        "-c",
        "user.email=torch@example.com",
        "commit",
        "-q",
        "--allow-empty",
        "-m",
        "init",
    ])
    .unwrap();
    assert!(commit.status.success(), "{commit:?}");
    let worktree = root.path().join("worktrees").join("feature");
    let added = run(&[
        "worktree",
        "add",
        "-q",
        "-b",
        "feature",
        worktree.to_str().unwrap(),
    ])
    .unwrap();
    assert!(added.status.success(), "{added:?}");
    let projects = vec![project(&repo)];
    assert_eq!(
        project_for_directory(&projects, &worktree),
        Some(&projects[0])
    );
}
