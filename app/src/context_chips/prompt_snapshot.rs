use itertools::Itertools;
use serde::{Deserialize, Serialize};
use torch_util::path::user_friendly_path;
use torchui::{AppContext, SingletonEntity};

use super::builtins::remote_login;
use super::current_prompt::CurrentPrompt;
use super::prompt::Prompt;
use super::{ChipResult, ChipValue, chips_to_string};
use crate::context_chips::ContextChipKind;
use crate::settings::WarpPromptSeparator;
use crate::terminal::model::block::Block;
use crate::terminal::model::session::Session;

/// Struct that holds a point in time snapshot of a prompt (chips are no longer interactive)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptSnapshot {
    chips: Vec<ChipResult>,

    same_line_prompt_enabled: bool,
    /// The separator to use as a trailing character at the end of Warp prompt, if any.
    separator: WarpPromptSeparator,
}

impl PromptSnapshot {
    pub fn from_current_prompt(current_prompt: &CurrentPrompt, ctx: &AppContext) -> Self {
        let prompt = Prompt::as_ref(ctx);
        let current_prompt_snapshot = current_prompt.snapshot();
        let current_prompt_on_click_snapshot = current_prompt.on_click_snapshot();

        // Get base chip kinds from prompt configuration
        let all_chip_kinds = prompt.chip_kinds();

        // Re-sort current prompt snapshot so that it matches the order of elements in prompt
        let chips = all_chip_kinds
            .iter()
            .map(|chip_kind| {
                let value = current_prompt_snapshot
                    .get(chip_kind)
                    .cloned()
                    .unwrap_or_default();
                let on_click_values = current_prompt_on_click_snapshot
                    .get(chip_kind)
                    .cloned()
                    .unwrap_or_default();
                ChipResult {
                    kind: chip_kind.clone(),
                    value,
                    on_click_values,
                }
            })
            .collect_vec();

        log::debug!("Current prompt snapshot: {chips:?}");
        Self {
            chips,
            same_line_prompt_enabled: current_prompt.same_line_prompt_enabled(),
            separator: current_prompt.separator(),
        }
    }

    pub fn from_chips(
        chips: Vec<ChipResult>,
        same_line_prompt_enabled: bool,
        separator: WarpPromptSeparator,
    ) -> Self {
        Self {
            chips,
            same_line_prompt_enabled,
            separator,
        }
    }

    /// A snapshot of where the shell reported it was for `block`: its environments, remote login,
    /// working directory and git HEAD.
    pub fn from_block(block: &Block, session: Option<&Session>) -> Self {
        let home_dir = session.and_then(Session::home_dir);
        let working_directory = block
            .pwd()
            .map(|pwd| user_friendly_path(pwd, home_dir).into_owned());
        let chips = [
            (
                ContextChipKind::CondaEnvironment,
                block.conda_env().cloned(),
            ),
            (
                ContextChipKind::VirtualEnvironment,
                block.virtual_env_short_name(),
            ),
            (ContextChipKind::Ssh, session.and_then(remote_login)),
            (ContextChipKind::WorkingDirectory, working_directory),
            (ContextChipKind::ShellGitBranch, block.git_branch().cloned()),
        ]
        .into_iter()
        .filter_map(|(kind, value)| {
            Some(ChipResult {
                kind,
                value: Some(ChipValue::Text(value?)),
                on_click_values: Vec::new(),
            })
        })
        .collect();
        Self {
            chips,
            same_line_prompt_enabled: false,
            separator: WarpPromptSeparator::None,
        }
    }

    /// The value of the given chip, in this snapshot.
    pub fn chip_value(&self, chip: &ContextChipKind) -> Option<ChipValue> {
        self.chips.iter().find_map(|chip_result| {
            if chip_result.kind == *chip {
                chip_result.value.clone()
            } else {
                None
            }
        })
    }

    pub(crate) fn chips(&self) -> &Vec<ChipResult> {
        &self.chips
    }

    pub(super) fn same_line_prompt_enabled(&self) -> bool {
        self.same_line_prompt_enabled
    }

    pub(super) fn separator(&self) -> WarpPromptSeparator {
        self.separator
    }
}

impl std::fmt::Display for PromptSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&chips_to_string(self.chips.clone().into_iter()))
    }
}
