use settings::Setting;
use torchui::elements::{
    Border, Container, CornerRadius, DropTarget, Element, Flex, Hoverable, ParentElement, Radius,
    SavePosition, Stack,
};
use torchui::{AppContext, SingletonEntity};

use super::Input;
use super::common::{
    add_command_xray_overlay, add_input_suggestions_overlays, add_vim_status_to_stack,
    add_voltron_overlay, add_workflow_info_overlay,
    wrap_input_with_terminal_padding_and_focus_handler,
};
use crate::appearance::Appearance;
use crate::context_chips::spacing;
use crate::features::FeatureFlag;
use crate::settings::{AppEditorSettings, InputModeSettings};
use crate::terminal::input::InputDropTargetData;
use crate::terminal::settings::TerminalSettings;
use crate::terminal::view::TerminalAction;
use crate::themes::theme::color::internal_colors;

impl Input {
    /// Renders the universal input. This is used when the user has 'universal' input type
    /// selected in settings.
    pub(super) fn render_universal_developer_input(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let menu_positioning = self.menu_positioning(app);

        let model = self.model.lock();

        // We should likely rework this stack to not need to use `with_constrain_absolute_children`,
        // by reworking the positioning of the children to not depend on this.
        let mut stack = Stack::new().with_constrain_absolute_children();

        let vim_state = self.editor.as_ref(app).vim_state(app);
        let app_editor_settings = AppEditorSettings::as_ref(app);
        let show_vim_status = vim_state.is_some() && *app_editor_settings.vim_status_bar.value();
        let _input_mode = *InputModeSettings::as_ref(app).input_mode.value();

        let mut column = Flex::column();

        let terminal_spacing = TerminalSettings::as_ref(app)
            .terminal_input_spacing(appearance.line_height_ratio(), app);
        column.add_child(
            Container::new(self.render_input_box(show_vim_status, appearance, app))
                .with_margin_top(
                    terminal_spacing.prompt_to_editor_padding
                        * spacing::UDI_PROMPT_BOTTOM_PADDING_FACTOR,
                )
                .finish(),
        );

        if let Some(vim_state) = vim_state.as_ref()
            && show_vim_status
        {
            add_vim_status_to_stack(
                &mut stack, vim_state, appearance, true, // use adjusted padding for UDI
            );
        }

        stack.add_child(wrap_input_with_terminal_padding_and_focus_handler(
            self.is_active_session(app),
            column.finish(),
            true, // use adjusted padding for UDI
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

        // If the file tree is enabled, don't include the top margin for UDI so that the UDI is flush with the
        // file tree.
        let margin_top = if FeatureFlag::FileTree.is_enabled() && self.is_input_at_top(&model, app)
        {
            0.
        } else {
            6.
        };

        // Wrap the stack in a container with background and border styling based on focus
        let mut container = Container::new(
            SavePosition::new(stack.finish(), &self.status_free_input_save_position_id()).finish(),
        )
        .with_margin_bottom(6.)
        .with_margin_left(6.)
        .with_margin_right(6.)
        .with_margin_top(margin_top)
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(8.)));

        // Apply styling based on focus state
        if self.is_pane_focused(app) {
            // Focused: show background
            container = container
                .with_background(if torch_core::channel::ChannelState::is_local_only() {
                    crate::themes::command_surface(theme)
                } else {
                    internal_colors::fg_overlay_1(theme)
                })
                .with_border(Border::all(1.).with_border_fill(theme.outline()));
        } else {
            // Unfocused: no background
            container = container.with_border(Border::all(1.).with_border_fill(theme.outline()));
        }

        let drop_target = DropTarget::new(
            container.finish(),
            InputDropTargetData::new(self.weak_view_handle.clone()),
        )
        .finish();

        let input = Hoverable::new(self.hoverable_handle.clone(), |_| drop_target)
            .on_middle_click(|ctx, _app, _position| {
                ctx.dispatch_typed_action(TerminalAction::MiddleClickOnInput)
            })
            .finish();

        SavePosition::new(input, &self.save_position_id()).finish()
    }
}
