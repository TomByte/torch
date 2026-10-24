use torch_core::ui::appearance::Appearance;
use torch_core::ui::color::coloru_with_opacity;
use torch_core::ui::theme::color::internal_colors;
use torch_core::ui::theme::{Fill, WarpTheme};
use torchui::Element;
use torchui::color::ColorU;
use torchui::elements::{ConstrainedBox, Container, CornerRadius, Radius};

use crate::ui_components::icons::Icon;

/// Padding around the status icon
pub const STATUS_ELEMENT_PADDING: f32 = 2.;

#[derive(Clone, Copy)]
pub enum StatusColorStyle {
    /// Foreground-blend colors (`ansi_fg`) used by the regular status badge.
    Standard,
    /// Background-blend colors (`ansi_bg`) used by the cloud overlay badge.
    Cloud,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConversationStatus {
    /// Agent is running.
    InProgress,

    /// The last turn of the agent finished with success.
    Success,

    /// The last turn of the agent completed with error.
    Error,

    /// The last turn failed transiently and an automatic recovery (retry or resume)
    /// is pending. Non-terminal: returns to `InProgress` when the recovery request
    /// sends, or falls to `Error` if recovery is exhausted.
    TransientError,

    /// The last turn of the agent was cancelled by the user.
    Cancelled,

    /// The last turn of the agent resulted in an action whose execution is blocked by the user.
    Blocked { blocked_action: String },

    /// Agent yielded via wait_for_events and is listening for inbound
    /// input. Quiescent but not terminal.
    WaitingForEvents,
}

impl std::fmt::Display for ConversationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversationStatus::InProgress => write!(f, "In progress"),
            ConversationStatus::Success => write!(f, "Done"),
            ConversationStatus::Error => write!(f, "Error"),
            ConversationStatus::TransientError => write!(f, "Reconnecting"),
            ConversationStatus::Cancelled => write!(f, "Cancelled"),
            ConversationStatus::Blocked { .. } => write!(f, "Blocked"),
            ConversationStatus::WaitingForEvents => write!(f, "Waiting"),
        }
    }
}

impl ConversationStatus {
    pub fn status_icon_and_color(
        &self,
        theme: &WarpTheme,
        color_style: StatusColorStyle,
    ) -> (Icon, ColorU) {
        match self {
            ConversationStatus::InProgress => (
                Icon::ClockLoader,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_magenta(),
                    StatusColorStyle::Cloud => theme.ansi_bg_magenta(),
                },
            ),
            ConversationStatus::Success => (
                Icon::Check,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_green(),
                    StatusColorStyle::Cloud => theme.ansi_bg_green(),
                },
            ),
            ConversationStatus::Error => (
                Icon::Triangle,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_red(),
                    StatusColorStyle::Cloud => theme.ansi_bg_red(),
                },
            ),
            ConversationStatus::TransientError => (
                Icon::ClockLoader,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_yellow(),
                    StatusColorStyle::Cloud => theme.ansi_bg_yellow(),
                },
            ),
            ConversationStatus::Cancelled => (Icon::StopFilled, internal_colors::neutral_5(theme)),
            ConversationStatus::Blocked { .. } => (
                Icon::StopFilled,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_yellow(),
                    StatusColorStyle::Cloud => theme.ansi_bg_yellow(),
                },
            ),
            ConversationStatus::WaitingForEvents => (
                Icon::ClockLoader,
                match color_style {
                    StatusColorStyle::Standard => theme.ansi_fg_magenta(),
                    StatusColorStyle::Cloud => theme.ansi_bg_magenta(),
                },
            ),
        }
    }
}

pub trait StatusElementStyle {
    fn status_icon_and_color(&self, theme: &WarpTheme) -> (Icon, ColorU);
}

impl StatusElementStyle for ConversationStatus {
    fn status_icon_and_color(&self, theme: &WarpTheme) -> (Icon, ColorU) {
        ConversationStatus::status_icon_and_color(self, theme, StatusColorStyle::Standard)
    }
}

/// Render the status element used by agent and conversation views.
pub fn render_status_element(
    status: &impl StatusElementStyle,
    icon_size: f32,
    appearance: &Appearance,
) -> Box<dyn Element> {
    let theme = appearance.theme();
    let (icon, color) = status.status_icon_and_color(theme);

    Container::new(
        ConstrainedBox::new(icon.to_warpui_icon(Fill::from(color)).finish())
            .with_width(icon_size)
            .with_height(icon_size)
            .finish(),
    )
    .with_uniform_padding(STATUS_ELEMENT_PADDING)
    .with_background(coloru_with_opacity(color, 10))
    .with_corner_radius(CornerRadius::with_all(Radius::Pixels(4.)))
    .finish()
}
