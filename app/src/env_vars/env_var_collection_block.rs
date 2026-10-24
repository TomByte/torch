use std::borrow::Cow;
use std::sync::Arc;

use lazy_static::lazy_static;
use parking_lot::RwLock;
use pathfinder_color::ColorU;
use settings::Setting as _;
use torch_core::features::FeatureFlag;
use torch_core::semantic_selection::SemanticSelection;
use torch_core::ui::Icon;
use torch_core::ui::theme::AnsiColorIdentifier;
use torchui::elements::{
    Border, Clipped, ConstrainedBox, Container, CornerRadius, CrossAxisAlignment, Expanded, Flex,
    FormattedTextElement, Hoverable, MainAxisAlignment, MainAxisSize, MouseStateHandle,
    ParentElement, Radius, SavePosition, SelectableArea, SelectionHandle, Shrinkable,
    SizeConstraintCondition, SizeConstraintSwitch, Text, get_rich_content_position_id,
};
use torchui::keymap::{FixedBinding, Keystroke};
use torchui::platform::Cursor;
use torchui::{
    AppContext, Element, Entity, EntityId, FocusContext, SingletonEntity, TypedActionView, View,
    ViewContext,
};

use crate::appearance::Appearance;
use crate::settings::InputModeSettings;
use crate::terminal::block_list_element::BlockListMenuSource;
use crate::terminal::block_list_viewport::InputMode;
use crate::terminal::view::TerminalAction;
use crate::ui_components::blended_colors;
use crate::ui_components::icons::Icon as UiIcon;
use crate::view_components::action_button::{
    ButtonSize, KeystrokeSource, NakedTheme, PrimaryTheme,
};
use crate::view_components::compactible_action_button::{
    CompactibleActionButton, RenderCompactibleActionButton, SMALL_SIZE_SWITCH_THRESHOLD,
    render_compact_and_regular_button_rows, render_expansion_icon,
};

const CONTENT_HORIZONTAL_PADDING: f32 = 20.;
const CONTENT_ITEM_VERTICAL_MARGIN: f32 = 16.;
const HEADER_HORIZONTAL_PADDING: f32 = 16.;
const HEADER_VERTICAL_PADDING: f32 = 10.;
const HEADER_ICON_MARGIN: f32 = 8.;

/// The vertical padding applied to the env var collection block's content body.
/// For horizontal padding, use [`HEADER_HORIZONTAL_PADDING`] for consistency.
const ENV_VAR_COLLECTION_BODY_VERTICAL_PADDING: f32 = 16.;

const ENV_VAR_COLLECTION_CANCEL_LABEL: &str = "Cancel";
const ENV_VAR_COLLECTION_ACCEPT_LABEL: &str = "Run";

lazy_static! {
    static ref CANCEL_ENV_VAR_COLLECTION_KEYSTROKE: Keystroke = Keystroke {
        ctrl: true,
        key: "c".to_owned(),
        ..Default::default()
    };
    static ref ACCEPT_ENV_VAR_COLLECTION_KEYSTROKE: Keystroke = Keystroke {
        key: "enter".to_owned(),
        ..Default::default()
    };
}

#[derive(Debug, Clone)]
pub enum EnvVarCollectionBlockEvent {
    Cancelled,
    RanCommand(String),
    ToggledExpanded(String),
    TextSelected,
}

#[derive(Debug, Clone)]
pub enum EnvVarCollectionBlockAction {
    Cancel,
    RunCommand,
    /// Only applies to text selections made at the `EnvVarCollectionBlock` level. Child views of the
    /// `EnvVarCollectionBlock` are responsible for managing their own text selection states.
    SelectText,
    ToggleExpanded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EnvVarCollectionState {
    /// The env var command is loaded and waiting to be run by the user.
    WaitingForUser,
    /// The env var command is currently running, after being accepted by the user.
    Running,
    /// The env var command finished running and succeeded.
    Succeeded,
    /// The env var command finished running and failed.
    Failed,
    /// The env var command was cancelled at some point before completing
    /// (i.e. before [`Self::Succeeded`] or [`Self::Failed`]).
    Cancelled,
}

impl EnvVarCollectionState {
    fn has_completed(&self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

pub struct EnvVarCollectionBlock {
    cancel_button: CompactibleActionButton,
    accept_button: CompactibleActionButton,

    command: String,
    command_output: Option<String>,
    state: EnvVarCollectionState,
    block_id: String,
    view_id: EntityId,

    /// The output grid needs to be selectable to allow users to copy the command to their clipboard.
    /// Only applies to text selections made at the `EnvVarCollectionBlock` level. Child views of the
    /// `EnvVarCollectionBlock` are responsible for managing their own text selection states.
    selection_handle: SelectionHandle,
    selected_text: Arc<RwLock<Option<String>>>,
    header_is_expanded: bool,
    header_mouse_state: MouseStateHandle,
}

pub fn init(app: &mut AppContext) {
    use torchui::keymap::macros::*;

    app.register_fixed_bindings([
        FixedBinding::new(
            "ctrl-c",
            EnvVarCollectionBlockAction::Cancel,
            id!(EnvVarCollectionBlock::ui_name()),
        ),
        FixedBinding::new(
            "enter",
            EnvVarCollectionBlockAction::RunCommand,
            id!(EnvVarCollectionBlock::ui_name()),
        ),
        FixedBinding::new(
            "numpadenter",
            EnvVarCollectionBlockAction::RunCommand,
            id!(EnvVarCollectionBlock::ui_name()),
        ),
    ]);
}

impl EnvVarCollectionBlock {
    pub fn new(
        block_id: String,
        _collection_title: String,
        command: String,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let cancel_button = CompactibleActionButton::new(
            ENV_VAR_COLLECTION_CANCEL_LABEL.to_string(),
            Some(KeystrokeSource::Fixed(
                CANCEL_ENV_VAR_COLLECTION_KEYSTROKE.clone(),
            )),
            ButtonSize::InlineActionHeader,
            EnvVarCollectionBlockAction::Cancel,
            Icon::X,
            Arc::new(NakedTheme),
            ctx,
        );

        let accept_button = CompactibleActionButton::new(
            ENV_VAR_COLLECTION_ACCEPT_LABEL.to_string(),
            Some(KeystrokeSource::Fixed(
                ACCEPT_ENV_VAR_COLLECTION_KEYSTROKE.clone(),
            )),
            ButtonSize::InlineActionHeader,
            EnvVarCollectionBlockAction::RunCommand,
            Icon::Check,
            Arc::new(PrimaryTheme),
            ctx,
        );

        Self {
            cancel_button,
            accept_button,
            command,
            command_output: None,
            state: EnvVarCollectionState::WaitingForUser,
            block_id,
            view_id: ctx.view_id(),
            selection_handle: Default::default(),
            selected_text: Default::default(),
            header_is_expanded: false,
            header_mouse_state: Default::default(),
        }
    }

    fn handle_toggle_expanded(&mut self, ctx: &mut ViewContext<Self>) {
        self.header_is_expanded = !self.header_is_expanded;

        ctx.emit(EnvVarCollectionBlockEvent::ToggledExpanded(
            self.block_id.clone(),
        ));
        ctx.notify();
    }

    pub fn focus(&self, ctx: &mut ViewContext<Self>) {
        ctx.focus_self();
    }

    pub fn get_block_id(&self) -> &String {
        &self.block_id
    }

    pub fn is_block_completed(&self) -> bool {
        self.state.has_completed()
    }

    pub fn is_running(&self) -> bool {
        self.state == EnvVarCollectionState::Running
    }

    pub fn on_succeeded(&mut self, ctx: &mut ViewContext<Self>) {
        self.state = EnvVarCollectionState::Succeeded;
        ctx.notify();
    }

    pub fn on_failed(&mut self, output: Option<String>, ctx: &mut ViewContext<Self>) {
        self.command_output = output;
        self.state = EnvVarCollectionState::Failed;
        ctx.notify();
    }

    fn run_command(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.is_block_completed() {
            self.state = EnvVarCollectionState::Running;
            ctx.emit(EnvVarCollectionBlockEvent::RanCommand(self.command.clone()));
            ctx.notify();
        }
    }

    pub fn cancel(&mut self, ctx: &mut ViewContext<Self>) {
        if !self.is_block_completed() {
            self.state = EnvVarCollectionState::Cancelled;
            ctx.emit(EnvVarCollectionBlockEvent::Cancelled);
            ctx.notify();
        }
    }

    /// Returns the currently selected text within the entire `EnvVarCollectionBlock` view sub-hierarchy.
    /// There **shouldn't** be more than one instance of selected text at any given time across
    /// any view within the same `EnvVarCollectionBlock` view sub-hierarchy.
    pub fn selected_text(&self, _ctx: &AppContext) -> Option<String> {
        self.selected_text.read().clone()
    }

    pub fn clear_selection(&mut self, ctx: &mut ViewContext<Self>) {
        self.selection_handle.clear();
        *self.selected_text.write() = None;
        ctx.notify();
    }

    pub fn handle_ctrl_c(&mut self, ctx: &mut ViewContext<Self>) {
        self.cancel(ctx);
    }

    fn render_header(&self, app: &AppContext) -> Box<dyn Element> {
        const COMMAND_WAITING_FOR_USER_MESSAGE: &str =
            "OK if I run this command and read the output?";

        let title: Cow<'static, str> = if self.state == EnvVarCollectionState::WaitingForUser {
            COMMAND_WAITING_FOR_USER_MESSAGE.into()
        } else {
            self.command.clone().into()
        };

        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let terminal_color = |color: AnsiColorIdentifier| {
            ColorU::from(color.to_ansi_color(&theme.terminal_colors().normal))
        };
        let (state_icon, state_icon_color) = match self.state {
            EnvVarCollectionState::WaitingForUser => (
                UiIcon::StopFilled,
                terminal_color(AnsiColorIdentifier::Yellow),
            ),
            EnvVarCollectionState::Running => {
                (UiIcon::Circle, terminal_color(AnsiColorIdentifier::Yellow))
            }
            EnvVarCollectionState::Succeeded => {
                (UiIcon::Check, terminal_color(AnsiColorIdentifier::Green))
            }
            EnvVarCollectionState::Failed => (UiIcon::X, terminal_color(AnsiColorIdentifier::Red)),
            EnvVarCollectionState::Cancelled => {
                (UiIcon::Cancelled, blended_colors::neutral_6(theme))
            }
        };
        let line_height = app.font_cache().line_height(
            appearance.monospace_font_size(),
            appearance.line_height_ratio(),
        );
        let expanded_downwards = self.state == EnvVarCollectionState::WaitingForUser
            || (self.state == EnvVarCollectionState::Failed && self.header_is_expanded);

        let header = |right_content: Option<Box<dyn Element>>| {
            let header_background = theme.surface_2();
            let icon = Container::new(
                ConstrainedBox::new(state_icon.to_warpui_icon(state_icon_color.into()).finish())
                    .with_width(line_height)
                    .with_height(line_height)
                    .finish(),
            )
            .with_margin_right(HEADER_ICON_MARGIN)
            .finish();
            let title_element = Text::new_inline(
                title.clone(),
                appearance.ui_font_family(),
                appearance.monospace_font_size(),
            )
            .soft_wrap(false)
            .with_selectable(true)
            .with_color(blended_colors::text_main(theme, header_background))
            .finish();
            let left_content = Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::Start)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(icon)
                .with_child(
                    Expanded::new(
                        1.,
                        Container::new(title_element).with_margin_right(8.).finish(),
                    )
                    .finish(),
                )
                .finish();
            let mut header_row = Flex::row()
                .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
                .with_main_axis_size(MainAxisSize::Max)
                .with_cross_axis_alignment(CrossAxisAlignment::Center)
                .with_child(Shrinkable::new(1., left_content).finish());
            if let Some(right_content) = right_content {
                header_row.add_child(right_content);
            }
            Container::new(header_row.finish())
                .with_padding_left(HEADER_HORIZONTAL_PADDING)
                .with_padding_right(HEADER_HORIZONTAL_PADDING)
                .with_vertical_padding(HEADER_VERTICAL_PADDING)
                .with_background(header_background)
                .with_corner_radius(if expanded_downwards {
                    CornerRadius::with_top(Radius::Pixels(8.))
                } else {
                    CornerRadius::with_all(Radius::Pixels(8.))
                })
                .finish()
        };

        match self.state {
            EnvVarCollectionState::WaitingForUser => {
                let buttons: Vec<&dyn RenderCompactibleActionButton> =
                    vec![&self.cancel_button, &self.accept_button];
                let (regular_row, compact_row) =
                    render_compact_and_regular_button_rows(buttons, None, appearance, app);
                SizeConstraintSwitch::new(
                    header(Some(regular_row)),
                    vec![(
                        SizeConstraintCondition::WidthLessThan(
                            SMALL_SIZE_SWITCH_THRESHOLD * appearance.monospace_ui_scalar(),
                        ),
                        header(Some(compact_row)),
                    )],
                )
                .finish()
            }
            EnvVarCollectionState::Failed => {
                let chevron = ConstrainedBox::new(render_expansion_icon(
                    self.header_is_expanded,
                    false,
                    appearance,
                    app,
                ))
                .with_height(line_height)
                .with_width(line_height)
                .finish();
                Hoverable::new(self.header_mouse_state.clone(), |_| header(Some(chevron)))
                    .on_click(|ctx, _, _| {
                        ctx.dispatch_typed_action(EnvVarCollectionBlockAction::ToggleExpanded);
                    })
                    .with_cursor(Cursor::PointingHand)
                    .finish()
            }
            EnvVarCollectionState::Running
            | EnvVarCollectionState::Succeeded
            | EnvVarCollectionState::Cancelled => header(None),
        }
    }
}

impl Entity for EnvVarCollectionBlock {
    type Event = EnvVarCollectionBlockEvent;
}

impl View for EnvVarCollectionBlock {
    fn ui_name() -> &'static str {
        "EnvVarCollectionBlock"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        // Build the stateless header based on current state
        let header_element = self.render_header(app);
        let mut content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Stretch)
            .with_child(Clipped::new(header_element).finish());

        let is_header_expanded =
            self.state == EnvVarCollectionState::WaitingForUser || self.header_is_expanded;
        let is_input_pinned_to_top =
            *InputModeSettings::as_ref(app).input_mode.value() == InputMode::PinnedToTop;

        // If we're expanding the env var collection block downward, we want the "Viewing command
        // detail" row to look connected to the block rendered below, which affects styling. Note
        // that `EnvVarCollectionState::Failed` is the only state that permits expansion toggling.
        let should_expand_downward = is_header_expanded
            && !is_input_pinned_to_top
            && self.state == EnvVarCollectionState::Failed;

        if is_header_expanded && self.state == EnvVarCollectionState::WaitingForUser {
            let selectable_child = Container::new(
                FormattedTextElement::from_str(
                    self.command.clone(),
                    appearance.monospace_font_family(),
                    appearance.monospace_font_size(),
                )
                .with_color(blended_colors::text_main(theme, theme.background()))
                .with_line_height_ratio(1.3)
                .set_selectable(true)
                .finish(),
            )
            .with_horizontal_padding(HEADER_HORIZONTAL_PADDING)
            .with_vertical_padding(ENV_VAR_COLLECTION_BODY_VERTICAL_PADDING)
            .with_background(theme.background())
            .with_corner_radius(CornerRadius::with_bottom(Radius::Pixels(8.)))
            .finish();

            let semantic_selection = SemanticSelection::as_ref(app);
            let selected_text = self.selected_text.clone();
            let view_id = self.view_id;

            let mut selectable = SelectableArea::new(
                self.selection_handle.clone(),
                move |selection_args, _, _| {
                    *selected_text.write() = selection_args.selection;
                },
                SavePosition::new(
                    selectable_child,
                    get_rich_content_position_id(&view_id).as_str(),
                )
                .finish(),
            )
            .with_word_boundaries_policy(semantic_selection.word_boundary_policy())
            .with_smart_select_fn(semantic_selection.smart_select_fn())
            .on_selection_updated(|ctx, _| {
                ctx.dispatch_typed_action(EnvVarCollectionBlockAction::SelectText)
            })
            .on_selection_right_click(move |ctx, position| {
                ctx.dispatch_typed_action(TerminalAction::BlockListContextMenu(
                    BlockListMenuSource::RichContentTextRightClick {
                        rich_content_view_id: view_id,
                        position_in_rich_content: position,
                    },
                ))
            });

            if FeatureFlag::RectSelection.is_enabled() {
                selectable = selectable.should_support_rect_select();
            }

            content.add_child(selectable.finish());
        }

        let border_color = if self.state == EnvVarCollectionState::WaitingForUser {
            theme.accent()
        } else {
            theme.surface_2()
        };

        let content = Container::new(content.finish())
            // Since expanded details are rendered using a regular block, having a non-zero horizontal
            // margin while toggled expanded will cause the body to look wider than the header.
            .with_horizontal_margin(if should_expand_downward {
                0.
            } else {
                CONTENT_HORIZONTAL_PADDING
            })
            // Since expanded details are rendered using a regular block, having a non-zero bottom
            // margin while toggled expanded will cause the body to look disconnected from the header.
            .with_margin_bottom(if should_expand_downward {
                0.
            } else {
                CONTENT_ITEM_VERTICAL_MARGIN
            })
            // Rounded corners will make the header feel disconnected from its expanded details.
            .with_corner_radius(if should_expand_downward {
                CornerRadius::with_top(Radius::Pixels(9.))
            } else {
                CornerRadius::with_all(Radius::Pixels(9.))
            })
            .with_border(Border::all(1.).with_border_fill(border_color))
            .finish();

        Container::new(content)
            .with_padding_top(CONTENT_ITEM_VERTICAL_MARGIN)
            .with_background(theme.ai_blocks_overlay())
            .with_border(Border::top(1.).with_border_fill(theme.outline()))
            .finish()
    }

    fn on_focus(&mut self, focus_ctx: &FocusContext, ctx: &mut ViewContext<Self>) {
        if focus_ctx.is_self_focused() {
            ctx.focus_self();
            ctx.notify();
        }
    }
}

impl TypedActionView for EnvVarCollectionBlock {
    type Action = EnvVarCollectionBlockAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            EnvVarCollectionBlockAction::RunCommand => self.run_command(ctx),
            EnvVarCollectionBlockAction::Cancel => self.cancel(ctx),
            EnvVarCollectionBlockAction::SelectText => {
                ctx.emit(EnvVarCollectionBlockEvent::TextSelected)
            }
            EnvVarCollectionBlockAction::ToggleExpanded => self.handle_toggle_expanded(ctx),
        }
    }
}
