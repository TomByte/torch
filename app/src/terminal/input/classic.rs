use pathfinder_geometry::vector::vec2f;
use settings::Setting;
use torchui::elements::{
    Border, ChildAnchor, ChildView, Container, DropTarget, Element, Empty, Flex, Hoverable,
    OffsetPositioning, ParentAnchor, ParentElement, ParentOffsetBounds, SavePosition, Stack,
};
use torchui::{AppContext, SingletonEntity};

use super::{Input, SubshellRenderState, should_render_prompt_using_editor_decorator_elements};
use crate::appearance::Appearance;
use crate::features::FeatureFlag;
use crate::settings::{AppEditorSettings, InputModeSettings};
use crate::terminal::block_list_viewport::InputMode;
use crate::terminal::input::common::{
    add_command_xray_overlay, add_input_suggestions_overlays, add_vim_status_to_stack,
    add_voltron_overlay, add_workflow_info_overlay,
    wrap_input_with_terminal_padding_and_focus_handler,
};
use crate::terminal::input::{InputDropTargetData, get_input_box_top_border_width};
use crate::terminal::settings::{SpacingMode, TerminalSettings};
use crate::terminal::view::TerminalAction;
use crate::terminal::warpify::render::{render_subshell_flag, render_subshell_flag_pole};

impl Input {
    /// Renders the classic input. This is used when the user has 'Honor PS1' enabled in settings.
    pub(super) fn render_classic_input(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let menu_positioning = self.menu_positioning(app);

        let model = self.model.lock();
        let should_render_prompt_using_editor_decorator_elements =
            should_render_prompt_using_editor_decorator_elements(
                false,
                self.is_cli_agent_input_open(app),
                &model,
                app,
            );

        // We should likely rework this stack to not need to use `with_constrain_absolute_children`,
        // by reworking the positioning of the children to not depend on this.
        let mut stack = Stack::new().with_constrain_absolute_children();

        let mut prompt_row = Stack::new();

        let (lprompt_top_area_option, rprompt_area_option);
        let mut prompt_top_padding_row = Stack::new();
        if should_render_prompt_using_editor_decorator_elements {
            // These are rendered as sections/notches in the EditorElement.
            lprompt_top_area_option = None;
            rprompt_area_option = None;

            let terminal_spacing = TerminalSettings::as_ref(app)
                .terminal_input_spacing(appearance.line_height_ratio(), app);
            let default_prompt_top_padding = terminal_spacing.block_padding.padding_top
                * self.size_info(app).cell_height_px().as_f32()
                - get_input_box_top_border_width();
            let prompt_top_padding_element = Container::new(Empty::new().finish())
                .with_padding_top(default_prompt_top_padding)
                .finish();
            prompt_top_padding_row.add_child(prompt_top_padding_element);
        } else {
            let prompt_elements = self
                .prompt_render_helper
                .render_prompt_areas(&model, appearance, app);
            lprompt_top_area_option = prompt_elements.lprompt;
            rprompt_area_option = prompt_elements.rprompt;
        }

        if !should_render_prompt_using_editor_decorator_elements {
            if let Some(lprompt_top_area) = lprompt_top_area_option {
                prompt_row.add_child(lprompt_top_area);
            }
            if let Some(rprompt_area) = rprompt_area_option {
                let block = &model.block_list().active_block();
                prompt_row.add_positioned_child(
                    rprompt_area,
                    OffsetPositioning::offset_from_parent(
                        block.rprompt_render_offset(&self.size_info(app)),
                        ParentOffsetBounds::Unbounded,
                        ParentAnchor::TopLeft,
                        ChildAnchor::TopLeft,
                    ),
                );
            }
        }

        let vim_state = self.editor.as_ref(app).vim_state(app);
        let app_editor_settings = AppEditorSettings::as_ref(app);
        let show_vim_status = vim_state.is_some() && *app_editor_settings.vim_status_bar.value();
        let input_mode = *InputModeSettings::as_ref(app).input_mode.value();

        let is_compact_mode = matches!(
            TerminalSettings::as_ref(app).spacing_mode.value(),
            SpacingMode::Compact
        );
        let mut column = Flex::column();

        column.add_children([prompt_top_padding_row.finish(), prompt_row.finish()]);

        column.add_child(self.render_input_box(show_vim_status, appearance, app));

        if !(matches!(input_mode, InputMode::PinnedToTop)
            && self
                .suggestions_mode_model
                .as_ref(app)
                .is_inline_menu_open())
        {
            column.add_child(
                Container::new(Flex::row().finish())
                    .with_margin_bottom(4.)
                    .finish(),
            );
        }

        let subshell_flag = self.get_subshell_flag_render_state(&model, is_compact_mode, app);

        let should_extend_flag = subshell_flag.is_some();

        if should_extend_flag {
            let max_height = self.size_info(app).pane_height_px().as_f32();
            stack.add_positioned_child(
                render_subshell_flag_pole(max_height, theme.subshell_background()),
                OffsetPositioning::offset_from_parent(
                    vec2f(0.0, 0.0),
                    ParentOffsetBounds::ParentBySize,
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(SubshellRenderState::Flag(command)) = subshell_flag {
            let flag = render_subshell_flag(
                command,
                appearance.monospace_font_family(),
                appearance.monospace_font_size(),
                theme,
            );
            stack.add_positioned_child(
                flag,
                OffsetPositioning::offset_from_parent(
                    vec2f(0.0, 0.0),
                    ParentOffsetBounds::Unbounded,
                    ParentAnchor::TopLeft,
                    ChildAnchor::TopLeft,
                ),
            );
        }

        if let Some(vim_state) = vim_state.as_ref()
            && show_vim_status
        {
            add_vim_status_to_stack(
                &mut stack, vim_state, appearance,
                false, // legacy doesn't use adjusted padding for vim status
            );
        }

        stack.add_child(wrap_input_with_terminal_padding_and_focus_handler(
            self.is_active_session(app),
            column.finish(),
            false, // legacy uses full padding
        ));

        if let Some(selected_workflow_state) = self.workflows_state.selected_workflow_state.as_ref()
            && selected_workflow_state.should_show_more_info_view
        {
            add_workflow_info_overlay(
                &mut stack,
                selected_workflow_state,
                self.size_info(app).pane_height_px().as_f32(),
                menu_positioning,
            );
        }

        if self.is_voltron_open && self.is_pane_focused(app) {
            add_voltron_overlay(&mut stack, &self.voltron_view, menu_positioning);
        }

        if self.is_pane_focused(app) {
            add_input_suggestions_overlays(self, &mut stack, appearance, menu_positioning, app);
        }

        if let Some(token_description) = &self.command_x_ray_description {
            add_command_xray_overlay(
                self,
                &mut stack,
                token_description,
                appearance,
                menu_positioning,
                app,
            );
        }

        let input_mode = *InputModeSettings::as_ref(app).input_mode.value();

        let border = match input_mode {
            InputMode::PinnedToBottom => {
                Border::top(get_input_box_top_border_width()).with_border_fill(theme.outline())
            }
            InputMode::PinnedToTop => {
                Border::bottom(get_input_box_top_border_width()).with_border_fill(theme.outline())
            }
            InputMode::Waterfall => Border::new(get_input_box_top_border_width())
                .with_sides(true, false, true, false)
                .with_border_fill(theme.outline()),
        };

        let drop_target = DropTarget::new(
            Container::new(stack.finish()).with_border(border).finish(),
            InputDropTargetData::new(self.weak_view_handle.clone()),
        )
        .finish();

        let input = SavePosition::new(
            Hoverable::new(self.hoverable_handle.clone(), |_| drop_target)
                .on_middle_click(|ctx, _app, _position| {
                    ctx.dispatch_typed_action(TerminalAction::MiddleClickOnInput)
                })
                .finish(),
            &self.status_free_input_save_position_id(),
        )
        .finish();

        let mut column = Flex::column();
        let is_inline_history_menu = FeatureFlag::InlineHistoryMenu.is_enabled()
            && self
                .suggestions_mode_model
                .as_ref(app)
                .is_inline_history_menu();
        let inline_history_menu =
            is_inline_history_menu.then(|| ChildView::new(&self.inline_history_menu_view).finish());

        match input_mode {
            InputMode::PinnedToBottom => {
                column.add_children(inline_history_menu.into_iter().chain([input]));
            }
            InputMode::PinnedToTop => {
                column.add_children([input].into_iter().chain(inline_history_menu));
            }
            InputMode::Waterfall => {
                let should_render_below = self
                    .inline_terminal_menu_positioner
                    .as_ref(app)
                    .should_render_inline_menu_below_input();

                if should_render_below {
                    column.add_children([input].into_iter().chain(inline_history_menu));
                } else {
                    column.add_children(inline_history_menu.into_iter().chain([input]));
                }
            }
        }

        SavePosition::new(column.finish(), &self.save_position_id()).finish()
    }
}
