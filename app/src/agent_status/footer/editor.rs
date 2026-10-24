//! Modal for customizing the agent input footer chip layout.
//!
//! Uses the shared [`ChipConfigurator`] with `LeftRightZones` layout to let users
//! drag/drop chips between left, right, and unused banks.

use settings::Setting as _;
use torch_errors::report_if_error;
use torchui::keymap::FixedBinding;
use torchui::{AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use super::toolbar_item::AgentToolbarItemKind;
use crate::Appearance;
use crate::appearance::AppearanceEvent;
use crate::chip_configurator::{
    ChipConfigurator, ChipConfiguratorAction, ChipConfiguratorLayout, ChipEditorModalConfig,
    ChipEditorMouseHandles, ChipEditorSectionsConfig, render_chip_editor_modal,
    render_chip_editor_sections,
};
use crate::terminal::session_settings::{
    CLIAgentToolbarChipSelection, SessionSettings, SessionSettingsChangedEvent,
    ToolbarChipSelection,
};

const MODAL_TITLE: &str = "Edit CLI agent toolbelt";

pub enum AgentToolbarEditorEvent {
    Close,
}

pub struct AgentToolbarEditorModal {
    mouse_handles: ChipEditorMouseHandles,
    chip_configurator: ChipConfigurator,
    is_dirty: bool,
}

pub struct AgentToolbarInlineEditor {
    mouse_handles: ChipEditorMouseHandles,
    chip_configurator: ChipConfigurator,
}

#[derive(Clone, Copy, Debug)]
pub enum AgentToolbarEditorAction {
    Cancel,
    Save,
    Chip(ChipConfiguratorAction),
    ResetDefault,
    /// Dummy action used as on_click for chip bank clicks (no-op).
    Activate,
}

#[derive(Clone, Copy, Debug)]
pub enum AgentToolbarInlineEditorAction {
    Chip(ChipConfiguratorAction),
    ResetDefault,
    /// Dummy action used as on_click for chip bank clicks (no-op).
    Activate,
}

fn open_toolbar_items_from_settings<V: View>(
    chip_configurator: &mut ChipConfigurator,
    ctx: &mut ViewContext<V>,
) {
    let appearance = Appearance::as_ref(ctx);
    let selection = SessionSettings::as_ref(ctx)
        .cli_agent_footer_chip_selection
        .clone();
    let current_left = selection.left_items();
    let current_right = selection.right_items();

    // Filter out items that are unavailable due to runtime state (user settings,
    // workspace config, etc.) on top of the feature-flag checks in all_available().
    let available: Vec<AgentToolbarItemKind> = AgentToolbarItemKind::all_available()
        .into_iter()
        .filter(|item| item.is_available(ctx))
        .collect();

    // Drop saved items that are no longer available (e.g. their feature flag was disabled
    // or a setting was turned off).
    let filter_unavailable = |items: Vec<AgentToolbarItemKind>| -> Vec<AgentToolbarItemKind> {
        items
            .into_iter()
            .filter(|item| available.contains(item))
            .collect()
    };
    let current_left = filter_unavailable(current_left);
    let current_right = filter_unavailable(current_right);

    chip_configurator.open_left_right_zones_with_items(
        current_left,
        current_right,
        available,
        appearance,
    );
}

fn open_default_toolbar_items<V: View>(
    chip_configurator: &mut ChipConfigurator,
    ctx: &mut ViewContext<V>,
) {
    let appearance = Appearance::as_ref(ctx);
    let left = AgentToolbarItemKind::default_left();
    let right = AgentToolbarItemKind::default_right();
    let available = AgentToolbarItemKind::all_available();
    let filter_runtime = |items: Vec<AgentToolbarItemKind>| -> Vec<AgentToolbarItemKind> {
        items
            .into_iter()
            .filter(|item| item.is_available(ctx))
            .collect()
    };
    let left = filter_runtime(left);
    let right = filter_runtime(right);
    let available = filter_runtime(available);
    chip_configurator.open_left_right_zones_with_items(left, right, available, appearance);
}

fn is_toolbar_editor_at_defaults(chip_configurator: &ChipConfigurator) -> bool {
    let left = chip_configurator.left_item_kinds();
    let right = chip_configurator.right_item_kinds();
    toolbar_items_match_defaults(&left, &right)
}

fn toolbar_items_match_defaults(
    left: &[AgentToolbarItemKind],
    right: &[AgentToolbarItemKind],
) -> bool {
    AgentToolbarItemKind::default_left().as_slice() == left
        && AgentToolbarItemKind::default_right().as_slice() == right
}

impl AgentToolbarInlineEditor {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let mut editor = Self {
            mouse_handles: Default::default(),
            chip_configurator: ChipConfigurator::new(ChipConfiguratorLayout::LeftRightZones),
        };
        editor.reset_from_settings(ctx);

        ctx.subscribe_to_model(&SessionSettings::handle(ctx), |me, _, event, ctx| {
            let should_refresh = matches!(
                event,
                SessionSettingsChangedEvent::CLIAgentToolbarChipSelectionSetting { .. }
            );

            if should_refresh && me.chip_configurator.current_dragging_state.is_none() {
                me.reset_from_settings(ctx);
                ctx.notify();
            }
        });

        // Chip colors are derived from the theme, so rebuild the chips from
        // settings when the theme changes to keep an open editor readable after a
        // theme switch (this inline editor persists its arrangement on every
        // edit, so reloading preserves the user's layout).
        ctx.subscribe_to_model(&Appearance::handle(ctx), |me, _, event, ctx| {
            if matches!(event, AppearanceEvent::ThemeChanged)
                && me.chip_configurator.current_dragging_state.is_none()
            {
                me.reset_from_settings(ctx);
                ctx.notify();
            }
        });

        editor
    }

    fn reset_from_settings(&mut self, ctx: &mut ViewContext<Self>) {
        open_toolbar_items_from_settings(&mut self.chip_configurator, ctx);
    }

    fn save_current_selection(&self, ctx: &mut ViewContext<Self>) {
        let left = self.chip_configurator.left_item_kinds();
        let right = self.chip_configurator.right_item_kinds();
        save_toolbar_selection(left, right, ctx);
    }

    fn is_at_defaults(&self) -> bool {
        is_toolbar_editor_at_defaults(&self.chip_configurator)
    }
}

impl Entity for AgentToolbarInlineEditor {
    type Event = ();
}

impl TypedActionView for AgentToolbarInlineEditor {
    type Action = AgentToolbarInlineEditorAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            Self::Action::Chip(chip_action) => {
                let should_save = self.chip_configurator.handle_action(chip_action, ctx);
                if should_save {
                    self.save_current_selection(ctx);
                }
                ctx.notify();
            }
            Self::Action::ResetDefault => {
                open_default_toolbar_items(&mut self.chip_configurator, ctx);
                self.save_current_selection(ctx);
                ctx.notify();
            }
            Self::Action::Activate => {
                // no-op — used as the on_click for chip bank items
            }
        }
    }
}

impl View for AgentToolbarInlineEditor {
    fn ui_name() -> &'static str {
        "AgentToolbarInlineEditor"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        render_chip_editor_sections(
            &self.chip_configurator,
            ChipEditorSectionsConfig {
                available_section_label: "Available chips",
                is_at_defaults: self.is_at_defaults(),
                reset_action: AgentToolbarInlineEditorAction::ResetDefault,
                activate_action: AgentToolbarInlineEditorAction::Activate,
                chip_action_wrapper: AgentToolbarInlineEditorAction::Chip,
                mouse_handles: &self.mouse_handles,
            },
            appearance,
        )
    }
}

pub fn init(app: &mut AppContext) {
    use torchui::keymap::macros::*;

    app.register_fixed_bindings([FixedBinding::new(
        "escape",
        AgentToolbarEditorAction::Cancel,
        id!(AgentToolbarEditorModal::ui_name()),
    )]);
}

fn save_toolbar_selection<V: View>(
    left: Vec<AgentToolbarItemKind>,
    right: Vec<AgentToolbarItemKind>,
    ctx: &mut ViewContext<V>,
) {
    let selection = if toolbar_items_match_defaults(&left, &right) {
        CLIAgentToolbarChipSelection::Default
    } else {
        CLIAgentToolbarChipSelection::Custom { left, right }
    };
    SessionSettings::handle(ctx).update(ctx, |settings, ctx| {
        report_if_error!(
            settings
                .cli_agent_footer_chip_selection
                .set_value(selection, ctx)
        );
    });
}

impl AgentToolbarEditorModal {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        // Chip colors are derived from the theme, so rebuild the chips when the
        // theme changes to keep an open editor readable after a theme switch.
        // Only rebuild while the modal is actually open (has chips) and not
        // mid-drag.
        ctx.subscribe_to_model(&Appearance::handle(ctx), |me, _, event, ctx| {
            if matches!(event, AppearanceEvent::ThemeChanged)
                && me.chip_configurator.current_dragging_state.is_none()
                && me.chip_configurator.has_items()
            {
                open_toolbar_items_from_settings(&mut me.chip_configurator, ctx);
                ctx.notify();
            }
        });
        Self {
            mouse_handles: Default::default(),
            chip_configurator: ChipConfigurator::new(ChipConfiguratorLayout::LeftRightZones),
            is_dirty: false,
        }
    }

    pub fn open(&mut self, ctx: &mut ViewContext<Self>) {
        self.reset();
        open_toolbar_items_from_settings(&mut self.chip_configurator, ctx);
        ctx.notify();
    }

    fn save_to_settings(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.is_dirty {
            return;
        }

        let left = self.chip_configurator.left_item_kinds();
        let right = self.chip_configurator.right_item_kinds();
        save_toolbar_selection(left, right, ctx);
    }

    fn reset(&mut self) {
        self.chip_configurator.reset();
        self.is_dirty = false;
    }
}

impl Entity for AgentToolbarEditorModal {
    type Event = AgentToolbarEditorEvent;
}

impl TypedActionView for AgentToolbarEditorModal {
    type Action = AgentToolbarEditorAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            Self::Action::Cancel => {
                self.reset();
                ctx.emit(AgentToolbarEditorEvent::Close);
            }
            Self::Action::Save => {
                self.save_to_settings(ctx);
                ctx.emit(AgentToolbarEditorEvent::Close);
            }
            Self::Action::Chip(chip_action) => {
                let mutated = self.chip_configurator.handle_action(chip_action, ctx);
                if mutated {
                    self.is_dirty = true;
                }
                ctx.notify();
            }
            Self::Action::ResetDefault => {
                self.is_dirty = true;
                open_default_toolbar_items(&mut self.chip_configurator, ctx);
                ctx.notify();
            }
            Self::Action::Activate => {
                // no-op — used as the on_click for chip bank items
            }
        }
    }
}

impl AgentToolbarEditorModal {
    fn is_at_defaults(&self) -> bool {
        is_toolbar_editor_at_defaults(&self.chip_configurator)
    }
}

impl View for AgentToolbarEditorModal {
    fn ui_name() -> &'static str {
        "AgentToolbarEditorModal"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        render_chip_editor_modal(
            &self.chip_configurator,
            ChipEditorModalConfig {
                title: MODAL_TITLE,
                available_section_label: "Available chips",
                is_at_defaults: self.is_at_defaults(),
                is_dirty: self.is_dirty,
                cancel_action: AgentToolbarEditorAction::Cancel,
                save_action: AgentToolbarEditorAction::Save,
                reset_action: AgentToolbarEditorAction::ResetDefault,
                activate_action: AgentToolbarEditorAction::Activate,
                chip_action_wrapper: AgentToolbarEditorAction::Chip,
                mouse_handles: &self.mouse_handles,
            },
            appearance,
        )
    }
}
