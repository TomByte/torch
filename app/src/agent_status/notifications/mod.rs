mod item;
mod item_rendering;
mod model;
pub(crate) mod toast_stack;
pub(crate) mod view;

pub(crate) use item::{
    NotificationCategory, NotificationFilter, NotificationId, NotificationItem, NotificationItems,
};
pub(crate) use model::{AgentNotificationsEvent, AgentNotificationsModel};

pub fn init(app: &mut torchui::AppContext) {
    view::NotificationMailboxView::init(app);
}
