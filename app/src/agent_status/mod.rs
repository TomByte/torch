pub(crate) mod conversation_status;
pub(crate) mod footer;
pub(crate) mod image_context;
pub(crate) mod input_config;
pub(crate) mod notifications;
pub(crate) mod review_context;

pub(crate) use conversation_status::{
    ConversationStatus, STATUS_ELEMENT_PADDING, StatusColorStyle, render_status_element,
};
pub(crate) use image_context::ImageContext;
pub(crate) use input_config::InputConfig;
pub(crate) use notifications::{AgentNotificationsEvent, AgentNotificationsModel};
pub(crate) use review_context::{AgentReviewCommentBatch, DiffSetHunk};

pub fn init(app: &mut torchui::AppContext) {
    footer::editor::init(app);
    notifications::init(app);
}
