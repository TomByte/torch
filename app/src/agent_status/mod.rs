pub(crate) mod conversation_status;
pub(crate) mod footer;
pub(crate) mod image_context;
pub(crate) mod notifications;
pub(crate) mod pending_images;
pub(crate) mod review_context;

pub(crate) use conversation_status::{
    ConversationStatus, STATUS_ELEMENT_PADDING, render_status_element,
};
pub(crate) use image_context::ImageContext;
pub(crate) use notifications::{AgentNotificationsEvent, AgentNotificationsModel};
pub(crate) use pending_images::PendingImages;
pub(crate) use review_context::{AgentReviewCommentBatch, DiffSetHunk};

pub fn init(app: &mut torchui::AppContext) {
    footer::editor::init(app);
    notifications::init(app);
}
