use torchui::{ModelHandle, ViewContext, ViewHandle};

use super::{CLIAgentFooter, CLIAgentFooterEvent};
use crate::context_chips::display_chip::{DisplayChip, GitLineChanges, PromptDisplayChipEvent};
use crate::context_chips::prompt_type::PromptType;
use crate::context_chips::{ChipResult, git_line_changes_from_chips};

impl CLIAgentFooter {
    /// Returns `true` if `DisplayChip`s should be recreated based on updated metadata values.
    ///
    /// This is basically cargo-culted from the equivalent logic in `PromptDisplay`, pared down to
    /// only the chip types that we care about in the CLI agent footer.
    fn check_if_chip_values_have_changed(
        existing_chips: &[ViewHandle<DisplayChip>],
        new_chips: &[ChipResult],
        ctx: &mut ViewContext<Self>,
    ) -> bool {
        existing_chips.len() != new_chips.len()
            || new_chips.iter().enumerate().any(|(i, chip_result)| {
                let existing_chip = &existing_chips[i];
                existing_chip.read(ctx, |chip, _| {
                    chip.value() != chip_result.value()
                        || chip.chip_kind() != chip_result.kind()
                        || chip.on_click_values() != chip_result.on_click_values()
                })
            })
    }

    fn create_display_chips(
        &self,
        new_chips: &[ChipResult],
        git_line_changes_info: Option<GitLineChanges>,
        ctx: &mut ViewContext<Self>,
    ) -> Vec<ViewHandle<DisplayChip>> {
        let mut display_chips = Vec::with_capacity(new_chips.len());
        let mut new_chips = new_chips.iter().peekable();
        while let Some(chip_result) = new_chips.next() {
            let next_chip_kind = new_chips
                .peek()
                .map(|chip_result| chip_result.kind().clone());

            let view_handle = ctx.add_typed_action_view(|ctx| {
                let config = self.display_chip_config.clone();
                let mut chip = DisplayChip::new_for_agent_view(
                    chip_result.clone(),
                    next_chip_kind,
                    config,
                    ctx,
                );
                chip.maybe_set_git_line_changes_info(git_line_changes_info.clone());
                chip.update_session_context(self.display_chip_config.session_context.clone(), ctx);
                chip
            });

            ctx.subscribe_to_view(&view_handle, move |_, _, event, ctx| match event {
                PromptDisplayChipEvent::ToggleMenu { open } => {
                    ctx.emit(CLIAgentFooterEvent::ToggledChipMenu { open: *open });
                    ctx.notify();
                }
                PromptDisplayChipEvent::TryExecuteCommand(cmd) => {
                    ctx.emit(CLIAgentFooterEvent::TryExecuteChipCommand(cmd.clone()));
                    ctx.notify();
                }
                PromptDisplayChipEvent::OpenCodeReview => {
                    ctx.emit(CLIAgentFooterEvent::OpenCodeReview);
                    ctx.notify();
                }
                _ => {
                    ctx.notify();
                }
            });

            display_chips.push(view_handle);
        }

        display_chips
    }

    fn update_existing_display_chips(
        display_chips: &[ViewHandle<DisplayChip>],
        git_line_changes_info: Option<GitLineChanges>,
        ctx: &mut ViewContext<Self>,
    ) {
        for chip_view in display_chips {
            chip_view.update(ctx, |chip, ctx| {
                chip.maybe_set_git_line_changes_info(git_line_changes_info.clone());
                ctx.notify();
            });
        }
    }

    /// Updates the display chip views based on a change to the underlying metadata that drives the
    /// prompt, modeled in `PromptType`.
    pub(super) fn update_display_chips(
        &mut self,
        model: &ModelHandle<PromptType>,
        ctx: &mut ViewContext<Self>,
    ) {
        let new_chips = model
            .as_ref(ctx)
            .cli_agent_chips(ctx)
            .into_iter()
            .filter(|chip_result| chip_result.value().is_some())
            .collect::<Vec<ChipResult>>();
        let git_line_changes_info = git_line_changes_from_chips(&new_chips);

        if Self::check_if_chip_values_have_changed(&self.cli_display_chips, &new_chips, ctx) {
            self.cli_display_chips =
                self.create_display_chips(&new_chips, git_line_changes_info, ctx);
        } else {
            Self::update_existing_display_chips(
                &self.cli_display_chips,
                git_line_changes_info,
                ctx,
            );
        }

        ctx.notify();
    }
}
