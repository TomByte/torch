use torchui::elements::{
    Align, ConstrainedBox, Container, CrossAxisAlignment, Expanded, Flex, ParentElement, Text,
};
use torchui::fonts::{Properties, Weight};
use torchui::{AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext};

use crate::appearance::Appearance;
use crate::auth::AuthStateProvider;
use crate::workspaces::user_workspaces::UserWorkspaces;

pub(crate) const TITLE: &str = "Cloud agents need a team";
pub(crate) const BODY_ADMIN: &str = "You’re in this workspace but not on a team, so you can’t start cloud runs. Join or create a team, then try again.";
pub(crate) const BODY_NON_ADMIN: &str = "You’re in this workspace but not on a team, so you can’t start cloud runs. Join a team, then try again.";
pub(crate) const TOAST_ADMIN: &str =
    "Cloud agents need a team. Join or create a team to start cloud runs.";
pub(crate) const TOAST_NON_ADMIN: &str =
    "Cloud agents need a team. Join a team to start cloud runs.";

fn is_current_user_workspace_admin(app: &AppContext) -> bool {
    let Some(email) = AuthStateProvider::as_ref(app).get().user_email() else {
        return false;
    };
    UserWorkspaces::as_ref(app)
        .current_workspace()
        .is_some_and(|workspace| workspace.is_workspace_admin(&email))
}

fn body_text(app: &AppContext) -> &'static str {
    if is_current_user_workspace_admin(app) {
        BODY_ADMIN
    } else {
        BODY_NON_ADMIN
    }
}

pub(crate) fn should_render(team_required: bool, is_in_setup: bool, is_configuring: bool) -> bool {
    team_required && (is_in_setup || is_configuring)
}

/// Toast copy for submissions blocked by the team requirement, mirroring the
/// admin/non-admin split of the inline blocker's body text.
pub(crate) fn toast_message(app: &AppContext) -> &'static str {
    if is_current_user_workspace_admin(app) {
        TOAST_ADMIN
    } else {
        TOAST_NON_ADMIN
    }
}

pub struct CloudAgentTeamRequiredView;

impl CloudAgentTeamRequiredView {
    pub fn new(_ctx: &mut ViewContext<Self>) -> Self {
        Self
    }
}

impl Entity for CloudAgentTeamRequiredView {
    type Event = ();
}

impl TypedActionView for CloudAgentTeamRequiredView {
    type Action = ();
}

impl View for CloudAgentTeamRequiredView {
    fn ui_name() -> &'static str {
        "CloudAgentTeamRequiredView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();

        let content = Flex::column()
            .with_cross_axis_alignment(CrossAxisAlignment::Start)
            .with_spacing(16.)
            .with_child(
                Text::new(TITLE, appearance.ui_font_family(), 20.)
                    .with_style(Properties::default().weight(Weight::Semibold))
                    .with_color(theme.active_ui_text_color().into_solid())
                    .finish(),
            )
            .with_child(
                Text::new(
                    body_text(app),
                    appearance.ui_font_family(),
                    appearance.ui_font_size(),
                )
                .with_color(theme.nonactive_ui_text_color().into_solid())
                .soft_wrap(true)
                .finish(),
            )
            .finish();

        Flex::column()
            .with_child(
                Expanded::new(
                    1.,
                    Container::new(
                        Align::new(ConstrainedBox::new(content).with_max_width(480.).finish())
                            .finish(),
                    )
                    .with_background(theme.background())
                    .with_uniform_padding(24.)
                    .finish(),
                )
                .finish(),
            )
            .finish()
    }
}
