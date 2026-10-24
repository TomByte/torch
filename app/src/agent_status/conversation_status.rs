use torch_core::ui::appearance::Appearance;
use torch_core::ui::color::coloru_with_opacity;
use torch_core::ui::theme::{Fill, WarpTheme};
use torchui::Element;
use torchui::color::ColorU;
use torchui::elements::{ConstrainedBox, Container, CornerRadius, Radius};

use crate::ui_components::icons::Icon;

/// Padding around the status icon
pub const STATUS_ELEMENT_PADDING: f32 = 2.;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ConversationStatus {
    /// Agent is running.
    InProgress,

    /// The last turn of the agent finished with success.
    Success,

    /// The last turn of the agent completed with error.
    Error,

    /// The last turn of the agent resulted in an action whose execution is blocked by the user.
    Blocked { blocked_action: String },
}

impl std::fmt::Display for ConversationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConversationStatus::InProgress => write!(f, "In progress"),
            ConversationStatus::Success => write!(f, "Done"),
            ConversationStatus::Error => write!(f, "Error"),
            ConversationStatus::Blocked { .. } => write!(f, "Blocked"),
        }
    }
}

impl ConversationStatus {
    pub fn status_icon_and_color(&self, theme: &WarpTheme) -> (Icon, ColorU) {
        match self {
            ConversationStatus::InProgress => (Icon::ClockLoader, theme.ansi_fg_magenta()),
            ConversationStatus::Success => (Icon::Check, theme.ansi_fg_green()),
            ConversationStatus::Error => (Icon::Triangle, theme.ansi_fg_red()),
            ConversationStatus::Blocked { .. } => (Icon::StopFilled, theme.ansi_fg_yellow()),
        }
    }
}

/// Render the status element used by agent and conversation views.
pub fn render_status_element(
    status: &ConversationStatus,
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
