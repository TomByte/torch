//! Local project folders and one-level project discovery.
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use settings::macros::define_settings_group;
use settings::{Setting as _, SupportedPlatforms, SyncToCloud};
use warpui::{Entity, ModelContext, SingletonEntity};

define_settings_group!(ProjectsSettings, settings: [
    directories: ProjectDirectories {
        type: Vec<PathBuf>, default: Vec::new(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "projects.directories",
        description: "Individual project folders available in the command palette.",
    },
    parent_directories: ProjectParentDirectories {
        type: Vec<PathBuf>, default: Vec::new(),
        supported_platforms: SupportedPlatforms::DESKTOP,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "projects.parent_directories",
        description: "Folders whose immediate subfolders are discovered as projects.",
    },
]);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalProject {
    pub path: PathBuf,
    pub name: String,
}

#[derive(Default)]
pub struct ProjectCatalog {
    pub projects: Vec<LocalProject>,
    pub unavailable: Vec<PathBuf>,
    generation: u64,
}

impl ProjectCatalog {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        ctx.subscribe_to_model(&ProjectsSettings::handle(ctx), |catalog, _, _, ctx| {
            catalog.refresh(ctx)
        });
        let mut catalog = Self::default();
        catalog.refresh(ctx);
        catalog
    }

    pub fn refresh(&mut self, ctx: &mut ModelContext<Self>) {
        let settings = ProjectsSettings::as_ref(ctx);
        let directories = settings.directories.value().clone();
        let parents = settings.parent_directories.value().clone();
        self.generation += 1;
        let generation = self.generation;
        ctx.spawn(
            async move { discover(&directories, &parents) },
            move |catalog, (projects, unavailable), ctx| {
                if generation == catalog.generation
                    && (projects != catalog.projects || unavailable != catalog.unavailable)
                {
                    catalog.projects = projects;
                    catalog.unavailable = unavailable;
                    ctx.notify();
                }
            },
        );
    }
}
impl Entity for ProjectCatalog {
    type Event = ();
}
impl SingletonEntity for ProjectCatalog {}

pub fn expand_project_path(path: &Path) -> PathBuf {
    if let Ok(rest) = path.strip_prefix("~")
        && let Some(home) = dirs::home_dir()
    {
        home.join(rest)
    } else {
        path.to_owned()
    }
}

/// The deepest project containing `directory`. A linked git worktree belongs to the project of
/// its repository's main working tree, wherever the worktree lives.
pub fn project_for_directory<'a>(
    projects: &'a [LocalProject],
    directory: &Path,
) -> Option<&'a LocalProject> {
    let directory = expand_project_path(directory);
    deepest_project_containing(projects, &directory).or_else(|| {
        let main_root = linked_worktree_main_root(&directory)?;
        deepest_project_containing(projects, &main_root)
    })
}

fn deepest_project_containing<'a>(
    projects: &'a [LocalProject],
    directory: &Path,
) -> Option<&'a LocalProject> {
    projects
        .iter()
        .filter(|project| directory.starts_with(&project.path))
        .max_by_key(|project| project.path.components().count())
}

/// Cached because sidebar grouping asks on every render; a directory's worktree never changes.
fn linked_worktree_main_root(directory: &Path) -> Option<PathBuf> {
    static MAIN_ROOTS: LazyLock<Mutex<HashMap<PathBuf, Option<PathBuf>>>> =
        LazyLock::new(Default::default);
    if let Some(cached) = MAIN_ROOTS
        .lock()
        .ok()
        .and_then(|roots| roots.get(directory).cloned())
    {
        return cached;
    }
    let main_root = find_linked_worktree_main_root(directory);
    if let Ok(mut roots) = MAIN_ROOTS.lock() {
        roots.insert(directory.to_owned(), main_root.clone());
    }
    main_root
}

/// Resolves what `git rev-parse --git-common-dir` reports without running git: a linked worktree's
/// `.git` file names its private git directory, whose `commondir` file leads to the main `.git`.
fn find_linked_worktree_main_root(directory: &Path) -> Option<PathBuf> {
    let dot_git = directory
        .ancestors()
        .map(|ancestor| ancestor.join(".git"))
        .find(|dot_git| dot_git.exists())?;
    if !dot_git.is_file() {
        return None;
    }
    let contents = std::fs::read_to_string(&dot_git).ok()?;
    let git_dir = dot_git
        .parent()?
        .join(contents.trim().strip_prefix("gitdir:")?.trim());
    let common_dir = std::fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common_dir = git_dir.join(common_dir.trim()).canonicalize().ok()?;
    // Bare repositories have no main working tree to group under.
    if common_dir.file_name()? != ".git" {
        return None;
    }
    common_dir.parent().map(Path::to_path_buf)
}

fn discover(directories: &[PathBuf], parents: &[PathBuf]) -> (Vec<LocalProject>, Vec<PathBuf>) {
    let mut paths = HashSet::new();
    let mut unavailable = Vec::new();
    for directory in directories {
        let path = expand_project_path(directory);
        match path.canonicalize() {
            Ok(path) if path.is_dir() => {
                paths.insert(path);
            }
            _ => unavailable.push(directory.clone()),
        }
    }
    for parent in parents {
        match std::fs::read_dir(expand_project_path(parent)) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    if entry.path().is_dir()
                        && let Ok(path) = entry.path().canonicalize()
                    {
                        paths.insert(path);
                    }
                }
            }
            Err(_) => unavailable.push(parent.clone()),
        }
    }
    let mut projects: Vec<_> = paths
        .into_iter()
        .map(|path| LocalProject {
            name: path
                .file_name()
                .unwrap_or(path.as_os_str())
                .to_string_lossy()
                .into_owned(),
            path,
        })
        .collect();
    projects.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.path.cmp(&b.path))
    });
    (projects, unavailable)
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
