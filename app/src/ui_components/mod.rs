//! Torch UI Components module contains functions and structs that implement our internal components
//! used for the apps design (our buttons with styling, headers and panels etc.) as well definition
//! of colors (aka blended colors from the figma designs derived from Torch theme) and icons used
//! within the app.
pub(crate) mod agent_icon;
pub(crate) mod avatar;
pub(crate) mod blended_colors;
pub mod buttons;
pub mod code_block;
pub(crate) mod color_dot;
pub(crate) mod dialog;
pub(crate) mod elevation;
pub(crate) mod icon_with_status;
pub(crate) mod item_highlight;
pub(crate) mod motion;
pub(crate) mod red_notification_dot;
pub(crate) mod render_file_search_row;
pub(crate) mod window_focus_dimming;

pub use torch_core::ui::icons;

const BORDER_RADIUS: f32 = 4.;
