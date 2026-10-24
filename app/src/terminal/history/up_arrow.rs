use std::collections::HashSet;

use torchui::{AppContext, SingletonEntity};

use super::History;
use crate::input_suggestions::HistoryInputSuggestion;
use crate::suggestions::ignored_suggestions_model::{IgnoredSuggestionsModel, SuggestionType};
use crate::terminal::model::session::SessionId;

fn sort_and_dedupe_suggestions<'a>(
    mut suggestions: Vec<HistoryInputSuggestion<'a>>,
    session_id: Option<SessionId>,
    all_live_session_ids: &HashSet<SessionId>,
) -> Vec<HistoryInputSuggestion<'a>> {
    suggestions.sort_by(|a, b| a.cmp(b, session_id, all_live_session_ids));

    // Keep the latest occurrence of each command.
    let mut seen_commands: HashSet<&str> = HashSet::new();
    let mut skip_indices: HashSet<usize> = HashSet::new();
    for (idx, suggestion) in suggestions.iter().enumerate().rev() {
        let text = suggestion.normalized_text();
        if text.is_empty() || !seen_commands.insert(text) {
            skip_indices.insert(idx);
        }
    }

    suggestions
        .into_iter()
        .enumerate()
        .filter(|(idx, _)| !skip_indices.contains(idx))
        .map(|(_, suggestion)| suggestion)
        .collect()
}

impl History {
    /// Returns de-duplicated command history ordered for up-arrow presentation, with commands
    /// from other sessions preceding commands from `session_id`.
    pub(crate) fn up_arrow_suggestions_for_terminal_surface<'a>(
        &'a self,
        session_id: Option<SessionId>,
        app: &'a AppContext,
    ) -> Vec<HistoryInputSuggestion<'a>> {
        let ignored_suggestions = app
            .has_singleton_model::<IgnoredSuggestionsModel>()
            .then(|| IgnoredSuggestionsModel::handle(app).as_ref(app));

        let commands = session_id
            .and_then(|session_id| self.commands(session_id))
            .unwrap_or_default()
            .into_iter()
            .filter(|entry| {
                ignored_suggestions.is_none_or(|ignored_suggestions| {
                    !ignored_suggestions.is_ignored(&entry.command, SuggestionType::ShellCommand)
                })
            })
            .map(|entry| HistoryInputSuggestion::Command { entry })
            .collect();

        sort_and_dedupe_suggestions(commands, session_id, &self.all_live_session_ids())
    }
}

#[cfg(test)]
#[path = "up_arrow_tests.rs"]
mod tests;
