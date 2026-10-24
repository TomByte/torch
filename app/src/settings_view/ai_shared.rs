//! Rendering helpers and style constants shared by the settings pages under
//! the Agents umbrella.
//!
//! These are generic over the action type so each page can dispatch its own
//! actions through them. The action parameters are `impl Action + Clone`
//! rather than named type parameters: several call sites turbofish the
//! `Setting` parameter, and an anonymous argument-position parameter keeps
//! those call sites working untouched.


use torch_core::ui::theme::color::internal_colors;
use torchui::elements::{
    ChildView, Container, Element, Fill, Flex, ParentElement,
};
use torchui::ui_components::components::{UiComponent, UiComponentStyles};
use torchui::ui_components::switch::SwitchStateHandle;
use torchui::{Action, AppContext, SingletonEntity, View, ViewContext, ViewHandle};

use super::SettingsAction;
use super::settings_page::{
    CONTENT_FONT_SIZE, ToggleState, build_toggle_element,
    render_body_item_label,
};
use crate::agent_status::footer::editor::AgentToolbarInlineEditor;
use crate::appearance::Appearance;
use crate::editor::{EditorView, InteractionState};

pub fn update_editor_interaction_state<V: View>(
    editor: ViewHandle<EditorView>,
    is_enabled: bool,
    ctx: &mut ViewContext<V>,
) {
    editor.update(ctx, |editor, ctx| {
        let interaction_state = if is_enabled {
            InteractionState::Editable
        } else {
            InteractionState::Disabled
        };
        editor.set_interaction_state(interaction_state, ctx);
        ctx.notify();
    })
}

/// The "Toolbar layout" chip editor. Shared by the Torch Agent toolbar and the
/// third-party coding agent toolbar, which are separate settings backed by the
/// same editor view.
pub fn render_toolbar_layout_editor(
    editor: &ViewHandle<AgentToolbarInlineEditor>,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let label = Container::new(
        appearance
            .ui_builder()
            .span("Toolbar layout".to_string())
            .with_style(UiComponentStyles {
                font_size: Some(CONTENT_FONT_SIZE),
                ..Default::default()
            })
            .build()
            .finish(),
    )
    .with_margin_bottom(4.)
    .finish();
    let editor = Container::new(ChildView::new(editor).finish())
        .with_margin_bottom(16.)
        .finish();

    Flex::column().with_child(label).with_child(editor).finish()
}

/// A settings row: label on the left, switch on the right.
pub fn render_ai_setting_toggle(
    label: impl Into<String>,
    action: impl Action + Clone,
    is_setting_enabled: bool,
    is_setting_toggleable: bool,
    switch_state: SwitchStateHandle,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    build_toggle_element(
        setting_label_element(label, is_setting_toggleable, app),
        render_ai_feature_switch(
            switch_state,
            is_setting_enabled,
            is_setting_toggleable,
            action,
            app,
        ),
        appearance,
        None,
    )
}

/// `render_body_item_label` is generic over an action type only to type its
/// optional click target. Settings labels never have one, so the parameter is
/// pinned here instead of being threaded through every caller.
fn setting_label_element(
    label: impl Into<String>,
    is_setting_toggleable: bool,
    app: &AppContext,
) -> Box<dyn Element> {
    render_body_item_label::<SettingsAction>(
        label.into(),
        Some(styles::header_font_color(is_setting_toggleable, app)),
        None,
        ToggleState::Enabled,
        Appearance::as_ref(app),
    )
}

pub fn render_ai_feature_switch(
    state_handle: SwitchStateHandle,
    is_setting_enabled: bool,
    is_setting_toggleable: bool,
    toggle_action: impl Action + Clone,
    app: &AppContext,
) -> Box<dyn Element> {
    let appearance = Appearance::as_ref(app);
    let ui_builder = appearance.ui_builder();
    ui_builder
        .switch(state_handle)
        .check(is_setting_enabled)
        .with_disabled(!is_setting_toggleable)
        .with_disabled_styles(UiComponentStyles {
            background: Some(Fill::Solid(internal_colors::neutral_4(appearance.theme()))),
            foreground: Some(Fill::Solid(internal_colors::neutral_5(appearance.theme()))),
            ..Default::default()
        })
        .build()
        .on_click(move |ctx, _, _| {
            if !is_setting_toggleable {
                return;
            }
            ctx.dispatch_typed_action(toggle_action.clone());
        })
        .finish()
}

pub mod styles {
    use torch_core::ui::appearance::Appearance;
    use torch_core::ui::theme::Fill;
    use torchui::{AppContext, SingletonEntity};

    /// Negative margin applied to description text so it appears closer to the main settings option
    /// text.
    pub const DESCRIPTION_NEGATIVE_MARGIN_OFFSET: f32 = -12.;

    /// The space between a description and the next toggle.
    pub const DESCRIPTION_MARGIN_BOTTOM: f32 = 12.;

    /// Margin to leave for switch toggle to the right of the description subtext.
    pub const TOGGLE_WIDTH_MARGIN: f32 = 48.;

    pub fn header_font_color(is_enabled_setting: bool, app: &AppContext) -> Fill {
        let appearance = Appearance::as_ref(app);
        if is_enabled_setting {
            appearance
                .theme()
                .main_text_color(appearance.theme().surface_2())
        } else {
            appearance.theme().disabled_ui_text_color()
        }
    }

    pub fn description_font_color(is_enabled_setting: bool, app: &AppContext) -> Fill {
        let appearance = Appearance::as_ref(app);
        if is_enabled_setting {
            appearance
                .theme()
                .sub_text_color(appearance.theme().surface_1())
        } else {
            appearance.theme().disabled_ui_text_color()
        }
    }
}
