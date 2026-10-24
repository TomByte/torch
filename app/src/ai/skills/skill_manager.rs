#[path = "file_watchers/mod.rs"]
mod file_watchers;
use std::collections::{HashMap, HashSet};

use ai::skills::{
    ParsedSkill, SkillPathOrigin, SkillProvider, SkillReference, SkillScope, provider_rank,
};
pub use file_watchers::{SkillWatcher, SkillWatcherEvent, extract_skill_parent_directory};
use torch_core::features::FeatureFlag;
use torch_util::host_id::HostId;
use torch_util::local_or_remote_path::LocalOrRemotePath;
use torchui::{AppContext, Entity, ModelContext, ModelHandle, SingletonEntity};

use super::bundled::{BundledSkill, BundledSkills};
use super::{ActiveSkillLookupError, SkillDescriptor, SkillManagerEvent, SkillPathQuery};
use crate::ai::skills::skill_utils::SkillDeduplicator;

pub struct SkillManager {
    /// Maps a directory path to the set of skill file paths defined in that directory.
    ///
    /// The key is the directory containing the `.agents/skills/` (or similar provider) folder,
    /// not the skills folder itself.
    ///
    /// Example: For a skill at `/repo/frontend/.agents/skills/deploy/SKILL.md`:
    /// - Key: `/repo/frontend`
    /// - Value (in the set): `/repo/frontend/.agents/skills/deploy/SKILL.md`
    ///
    /// NOT:
    /// - Key: `/repo/frontend/.agents/skills`
    directory_skills: HashMap<LocalOrRemotePath, HashSet<LocalOrRemotePath>>,
    skills_by_path: HashMap<LocalOrRemotePath, ParsedSkill>,
    /// Reverse lookup: skill name → set of paths with that name.
    /// This allows efficient lookup by skill name without scanning all paths.
    skills_by_name: HashMap<String, HashSet<LocalOrRemotePath>>,
    /// Skills bundled into Torch for the local host and connected remote hosts.
    bundled_skills: BundledSkills,
    /// Home directories published by connected remote hosts.
    ///
    /// Remote home skills themselves live in the shared file-skill indexes above,
    /// alongside local home and project skills.
    remote_home_directories: HashMap<HostId, LocalOrRemotePath>,
    /// When true, all skills in `directory_skills` are in scope regardless of
    /// the current working directory. Set by `AgentDriver` when a cloud
    /// environment with configured repos is active, so the agent sees every
    /// skill from every cloned repo.
    is_cloud_environment: bool,
    #[allow(dead_code)]
    skill_watcher: ModelHandle<SkillWatcher>, // Can't remove this or it'll get cleaned up after new()
}

impl SkillManager {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let (skill_watcher_tx, skill_watcher_rx) = async_channel::unbounded();

        ctx.spawn_stream_local(
            skill_watcher_rx,
            |me, message, ctx| {
                me.handle_skill_watcher_event(message, ctx);
            },
            |_, _| {}, // No cleanup needed when stream ends
        );

        // Create skill watcher
        let skill_watcher = ctx.add_model(|ctx| SkillWatcher::new(ctx, skill_watcher_tx));

        if FeatureFlag::BundledSkills.is_enabled() {
            ctx.spawn(BundledSkill::detect(), |me, result, _| {
                me.bundled_skills.set_local(result);
            });
        }

        Self {
            directory_skills: HashMap::new(),
            skills_by_path: HashMap::new(),
            skills_by_name: HashMap::new(),
            bundled_skills: BundledSkills::default(),
            remote_home_directories: HashMap::new(),
            is_cloud_environment: false,
            skill_watcher,
        }
    }

    /// Returns skills available for the given working directory.
    pub fn get_skills_for_working_directory(
        &self,
        working_directory: Option<&LocalOrRemotePath>,
        ctx: &AppContext,
    ) -> Vec<SkillDescriptor> {
        let path_origin = match working_directory {
            Some(LocalOrRemotePath::Remote(path)) => SkillPathOrigin::Remote {
                host_id: path.host_id.clone(),
            },
            Some(LocalOrRemotePath::Local(_)) | None => SkillPathOrigin::Local,
        };
        self.get_skills_for_working_directory_with_origin(working_directory, &path_origin, ctx)
    }

    /// Returns skills available for the given working directory and execution host.
    pub fn get_skills_for_working_directory_with_origin(
        &self,
        working_directory: Option<&LocalOrRemotePath>,
        path_origin: &SkillPathOrigin,
        ctx: &AppContext,
    ) -> Vec<SkillDescriptor> {
        // Collect file-backed skills for one shared deduplication pass. Home skills use
        // the home directory as their dir_path; project skills use their owning directory.
        let mut skill_paths = Vec::new();
        let mut deduplicator = SkillDeduplicator::default();
        let path_matches_location = |path: &LocalOrRemotePath| match (working_directory, path) {
            (Some(LocalOrRemotePath::Local(_)), LocalOrRemotePath::Local(_)) => true,
            (
                Some(LocalOrRemotePath::Remote(working_directory)),
                LocalOrRemotePath::Remote(path),
            ) => working_directory.host_id == path.host_id,
            (None, LocalOrRemotePath::Local(_)) => self.is_cloud_environment,
            (Some(LocalOrRemotePath::Local(_)), LocalOrRemotePath::Remote(_))
            | (Some(LocalOrRemotePath::Remote(_)), LocalOrRemotePath::Local(_))
            | (None, LocalOrRemotePath::Remote(_)) => false,
        };

        if let Some(home_dir) = self.home_directory_for_origin(path_origin)
            && let Some(home_skill_paths) = self.directory_skills.get(&home_dir)
        {
            skill_paths.extend(
                home_skill_paths
                    .iter()
                    .cloned()
                    .map(|path| (home_dir.clone(), path)),
            );
        }

        if self.is_cloud_environment {
            // In cloud environments, all skills in the working directory's location are in scope
            // regardless of cwd.
            for (dir, dir_skill_paths) in &self.directory_skills {
                if self.is_home_directory(dir) || !path_matches_location(dir) {
                    continue;
                }
                for path in dir_skill_paths {
                    skill_paths.push((dir.clone(), path.clone()));
                }
            }
        } else if let Some(working_directory) = working_directory {
            let repo_root = repo_metadata::repositories::DetectedRepositories::as_ref(ctx)
                .get_root_for_path(working_directory);

            for (dir, dir_skill_paths) in &self.directory_skills {
                if self.is_home_directory(dir) {
                    continue;
                }
                // Only include skills from directories that are ancestors of the working directory
                // (or the working directory itself)
                if working_directory.starts_with(dir) {
                    // Also verify this directory is within the detected repo (if any)
                    if repo_root.as_ref().is_none_or(|root| dir.starts_with(root)) {
                        for path in dir_skill_paths {
                            skill_paths.push((dir.clone(), path.clone()));
                        }
                    }
                }
            }
        }

        // Deduplicate skills with identical content installed under the same directory across
        // multiple providers, keeping the skill from the highest-priority provider per
        // [`SKILL_PROVIDER_DEFINITIONS`].
        deduplicator.extend_paths(&skill_paths, &self.skills_by_path);
        let mut skills = deduplicator.into_descriptors();

        // Apply icon overrides for well-known skill names (e.g. partner integrations).
        for skill in &mut skills {
            if skill.icon_override.is_none() {
                skill.icon_override =
                    crate::ai::skills::skill_utils::icon_override_for_skill_name(&skill.name);
            }
        }

        // Append bundled skills whose activation condition is met, from the
        // catalog of the active execution host: SSH sessions see the remote
        // daemon's catalog (empty until its snapshot arrives),
        // never the local client's. Remote catalog descriptors are referenced
        // by their remote paths so invocation resolves back to the same host's
        // catalog, while direct `BundledSkillId` lookups use `path_origin`.
        if FeatureFlag::BundledSkills.is_enabled() {
            skills.extend(self.bundled_skills.active_descriptors(path_origin, ctx));
        }

        skills
    }

    /// Returns the parsed home skills currently cached by the local watcher.
    pub fn home_skills(&self) -> impl Iterator<Item = &ParsedSkill> + '_ {
        dirs::home_dir()
            .map(LocalOrRemotePath::Local)
            .into_iter()
            .filter_map(|home_dir| self.directory_skills.get(&home_dir))
            .flatten()
            .filter_map(|path| self.skills_by_path.get(path))
    }

    /// Returns true if the skill (or any of its provider-path variants) exists in
    /// a folder matching one of the given `providers`. This handles the deduplication
    /// edge case where a skill is present in multiple provider folders (e.g. both
    /// `.agents/skills/` and `.claude/skills/`) but deduplication picked a provider
    /// that the caller doesn't support.
    pub fn skill_exists_for_any_provider(
        &self,
        skill: &SkillDescriptor,
        providers: &[SkillProvider],
    ) -> bool {
        // Fast path: the deduplicated provider already matches.
        if providers.contains(&skill.provider) {
            return true;
        }
        // Slow path: check all paths for this skill name.
        self.providers_for_descriptor(skill)
            .any(|provider| providers.contains(&provider))
    }

    /// Returns the best supported provider for a skill given a set of supported providers.
    ///
    /// When a skill is duplicated across multiple provider folders (e.g. both
    /// `.agents/skills/` and `.claude/skills/`), the global deduplication picks the
    /// highest-priority provider per [`SKILL_PROVIDER_DEFINITIONS`]. However, for the
    /// CLI agent footer `/skills` menu we want the icon to reflect the provider that
    /// the active CLI agent actually supports.
    ///
    /// This method checks all paths for the skill name and returns the supported
    /// provider with the best (lowest) rank. Falls back to the skill's deduped
    /// provider if no supported provider is found among its paths.
    pub fn best_supported_provider(
        &self,
        skill: &SkillDescriptor,
        supported_providers: &[SkillProvider],
    ) -> SkillProvider {
        // Fast path: the deduplicated provider is already supported.
        if supported_providers.contains(&skill.provider) {
            return skill.provider;
        }
        // Find the supported provider with the best (lowest) rank among all paths.
        self.providers_for_descriptor(skill)
            .filter(|provider| supported_providers.contains(provider))
            .min_by_key(|provider| provider_rank(*provider))
            .unwrap_or(skill.provider)
    }

    fn providers_for_descriptor<'a>(
        &'a self,
        descriptor: &'a SkillDescriptor,
    ) -> impl Iterator<Item = SkillProvider> + 'a {
        self.skills_by_name
            .get(&descriptor.name)
            .into_iter()
            .flatten()
            .filter(|path| path_matches_reference_location(path, &descriptor.reference))
            .filter_map(|path| self.skills_by_path.get(path).map(|skill| skill.provider))
    }

    /// Returns a reference to a parsed skill for a specific SKILL.md file path, if it is cached.
    /// Falls through to the remote bundled catalog, whose skills are addressed by path.
    pub fn skill_by_path<P: SkillPathQuery + ?Sized>(
        &self,
        skill_path: &P,
    ) -> Option<&ParsedSkill> {
        let location = skill_path.to_skill_location();
        self.skill_by_location(&location)
    }

    /// Returns the appropriate `SkillReference` for a skill at the given path.
    /// For bundled skills, returns `BundledSkillId`; otherwise returns `Path`.
    pub fn reference_for_skill_path<P: SkillPathQuery + ?Sized>(
        &self,
        skill_path: &P,
    ) -> SkillReference {
        let skill_path = skill_path.to_skill_location();
        // Check if this path belongs to a bundled skill.
        if let Some(reference) = self.bundled_skills.reference_for_path(&skill_path) {
            return reference;
        }
        // Default to path-based reference.
        SkillReference::Path(skill_path)
    }

    /// Get the definition of a skill, if it is cached.
    pub fn skill_by_reference(&self, reference: &SkillReference) -> Option<&ParsedSkill> {
        match reference {
            SkillReference::Path(path) => self.skill_by_location(path),
            SkillReference::BundledSkillId(id) => self.bundled_skills.local_skill(id),
        }
    }

    /// Get the definition of a skill for the selected execution host only if it is active.
    pub fn active_skill_by_reference_with_origin(
        &self,
        reference: &SkillReference,
        path_origin: &SkillPathOrigin,
        ctx: &AppContext,
    ) -> Result<&ParsedSkill, ActiveSkillLookupError> {
        let skill = match reference {
            SkillReference::Path(path) => self.skills_by_path.get(path).or_else(|| {
                let remote = path.as_remote()?;
                let SkillPathOrigin::Remote { host_id } = path_origin else {
                    return None;
                };
                if remote.host_id != *host_id {
                    return None;
                }
                self.bundled_skills.remote_active_skill_by_path(remote, ctx)
            }),
            SkillReference::BundledSkillId(id) => {
                self.bundled_skills.active_skill(id, path_origin, ctx)
            }
        };
        skill.ok_or_else(|| ActiveSkillLookupError::for_reference(reference, path_origin))
    }

    /// Returns a local bundled skill by ID only if its activation condition is met.
    pub fn active_local_bundled_skill(&self, id: &str, ctx: &AppContext) -> Option<&ParsedSkill> {
        self.bundled_skills
            .active_skill(id, &SkillPathOrigin::Local, ctx)
    }

    fn home_directory_for_origin(
        &self,
        path_origin: &SkillPathOrigin,
    ) -> Option<LocalOrRemotePath> {
        match path_origin {
            SkillPathOrigin::Local => dirs::home_dir().map(LocalOrRemotePath::Local),
            SkillPathOrigin::Remote { host_id } => {
                self.remote_home_directories.get(host_id).cloned()
            }
            SkillPathOrigin::RestoredDisplayOnly | SkillPathOrigin::Unavailable => None,
        }
    }

    fn is_home_directory(&self, path: &LocalOrRemotePath) -> bool {
        match path {
            LocalOrRemotePath::Local(path) => dirs::home_dir().as_ref() == Some(path),
            LocalOrRemotePath::Remote(remote_path) => self
                .remote_home_directories
                .get(&remote_path.host_id)
                .is_some_and(|home_dir| home_dir == path),
        }
    }

    fn skill_by_location(&self, location: &LocalOrRemotePath) -> Option<&ParsedSkill> {
        self.skills_by_path.get(location).or_else(|| {
            location
                .as_remote()
                .and_then(|remote| self.bundled_skills.remote_skill_by_path(remote))
        })
    }

    fn remove_skill_by_path(&mut self, skill_path: &LocalOrRemotePath) {
        let Some(skill) = self.skills_by_path.remove(skill_path) else {
            return;
        };
        let remove_name = self
            .skills_by_name
            .get_mut(&skill.name)
            .is_some_and(|paths| {
                paths.remove(skill_path);
                paths.is_empty()
            });
        if remove_name {
            self.skills_by_name.remove(&skill.name);
        }
    }

    fn handle_skill_watcher_event(
        &mut self,
        event: SkillWatcherEvent,
        ctx: &mut ModelContext<Self>,
    ) {
        let home_skills_changed = match &event {
            SkillWatcherEvent::SkillsAdded { skills } => {
                skills.iter().any(|skill| skill.scope == SkillScope::Home)
            }
            SkillWatcherEvent::SkillsDeleted { paths } => paths.iter().any(|path| {
                self.skills_by_path.values().any(|skill| {
                    skill.scope == SkillScope::Home
                        && (skill.path.starts_with(path) || path.starts_with(&skill.path))
                })
            }),
        };
        match event {
            SkillWatcherEvent::SkillsAdded { skills } => {
                self.handle_skills_added(skills);
            }
            SkillWatcherEvent::SkillsDeleted { paths } => {
                self.handle_skills_deleted(paths);
            }
        }
        ctx.emit(SkillManagerEvent::SkillsChanged {
            home_skills_changed,
        });
    }

    pub fn handle_skills_added(&mut self, skills: Vec<ParsedSkill>) {
        for skill in skills {
            match extract_skill_parent_directory(&skill.path) {
                Ok(parent_dir) => {
                    self.directory_skills
                        .entry(parent_dir)
                        .or_default()
                        .insert(skill.path.clone());

                    self.skills_by_name
                        .entry(skill.name.clone())
                        .or_default()
                        .insert(skill.path.clone());
                    self.skills_by_path.insert(skill.path.clone(), skill);
                }
                _ => {
                    log::warn!(
                        "Could not extract parent directory for skill: {:?}",
                        skill.path
                    );
                }
            }
        }
    }

    fn handle_skills_deleted(&mut self, paths: Vec<LocalOrRemotePath>) {
        for path in paths {
            self.handle_path_deleted(&path);
        }
    }

    fn handle_path_deleted(&mut self, path: &LocalOrRemotePath) {
        // Delete all skills that are affected by this deleted path
        for (dir, skill_paths) in &self.directory_skills.clone() {
            if dir.starts_with(path) {
                // Delete this entire entry and remove all skill_paths under this directory from cache
                for skill_path in skill_paths {
                    self.remove_skill_by_path(skill_path);
                }
                self.directory_skills.remove(dir);
            } else if path.starts_with(dir) {
                // Remove all skills under this directory that is a child of the deleted path
                for skill_path in skill_paths {
                    if skill_path.starts_with(path) {
                        self.remove_skill_by_path(skill_path);
                        self.directory_skills
                            .entry(dir.clone())
                            .or_default()
                            .remove(skill_path);
                    }
                }
            }
        }
    }
}
impl Entity for SkillManager {
    type Event = SkillManagerEvent;
}

impl SingletonEntity for SkillManager {}

fn path_matches_reference_location(path: &LocalOrRemotePath, reference: &SkillReference) -> bool {
    match (path, reference) {
        (
            LocalOrRemotePath::Remote(path),
            SkillReference::Path(LocalOrRemotePath::Remote(reference)),
        ) => path.host_id == reference.host_id,
        (
            LocalOrRemotePath::Local(_),
            SkillReference::Path(LocalOrRemotePath::Local(_)) | SkillReference::BundledSkillId(_),
        ) => true,
        (LocalOrRemotePath::Local(_), SkillReference::Path(LocalOrRemotePath::Remote(_)))
        | (
            LocalOrRemotePath::Remote(_),
            SkillReference::Path(LocalOrRemotePath::Local(_)) | SkillReference::BundledSkillId(_),
        ) => false,
    }
}
