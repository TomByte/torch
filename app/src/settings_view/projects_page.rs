//! Local project configuration.
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use settings::Setting as _;
use warpui::elements::{
    Align, Container, CrossAxisAlignment, Expanded, Flex, MouseStateHandle, ParentElement,
    Shrinkable, Text,
};
use warpui::platform::FilePickerConfiguration;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::{UiComponent, UiComponentStyles};
use warpui::{
    AppContext, Element, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle,
};

use super::settings_page::{
    HEADER_PADDING, LocalOnlyIconState, MatchData, PageTitle, PageType, SettingsPageMeta,
    SettingsPageViewHandle, SettingsWidget, TOGGLE_BUTTON_RIGHT_PADDING, render_body_item_label,
};
use super::{SettingsSection, ToggleState};
use crate::appearance::Appearance;
use crate::settings::{ProjectCatalog, ProjectsSettings};

#[derive(Clone, Debug, PartialEq)]
pub enum ProjectsPageAction {
    Add { parent: bool },
    Remove { parent: bool, path: PathBuf },
}

pub struct ProjectsPageView {
    page: PageType<Self>,
    remove_states: RefCell<HashMap<(bool, PathBuf), MouseStateHandle>>,
    error: Option<String>,
}
impl ProjectsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        ctx.subscribe_to_model(&ProjectsSettings::handle(ctx), |view, _, _, ctx| {
            view.remove_states.borrow_mut().clear();
            ctx.notify();
        });
        ctx.observe(&ProjectCatalog::handle(ctx), |_, _, ctx| ctx.notify());
        let mut page = PageType::new_uncategorized(
            vec![
                Box::new(LocationsWidget::<false>::default()),
                Box::new(LocationsWidget::<true>::default()),
            ],
            Some(PageTitle::new("Projects")),
        );
        page.set_min_page_width(0.);
        Self {
            page,
            remove_states: RefCell::new(HashMap::new()),
            error: None,
        }
    }
    fn change(&mut self, parent: bool, path: PathBuf, add: bool, ctx: &mut ViewContext<Self>) {
        let result = ProjectsSettings::handle(ctx).update(ctx, |settings, ctx| {
            let mut paths = if parent {
                settings.parent_directories.value().clone()
            } else {
                settings.directories.value().clone()
            };
            if add {
                if !paths.contains(&path) {
                    paths.push(path);
                }
            } else {
                paths.retain(|existing| existing != &path);
            }
            if parent {
                settings.parent_directories.set_value(paths, ctx)
            } else {
                settings.directories.set_value(paths, ctx)
            }
        });
        self.error = result
            .err()
            .map(|error| format!("Could not save projects: {error}"));
        ctx.notify();
    }
}
impl Entity for ProjectsPageView {
    type Event = ();
}
impl TypedActionView for ProjectsPageView {
    type Action = ProjectsPageAction;
    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ProjectsPageAction::Add { parent } => {
                let parent = *parent;
                ctx.open_file_picker(
                    move |result, ctx| {
                        if let Ok(paths) = result
                            && let Some(path) = paths.into_iter().next()
                            && let Some(handle) = ctx.handle().upgrade(ctx)
                        {
                            handle.update(ctx, |view, ctx| {
                                view.change(parent, path.into(), true, ctx)
                            });
                        }
                    },
                    FilePickerConfiguration::new().folders_only(),
                );
            }
            ProjectsPageAction::Remove { parent, path } => {
                self.change(*parent, path.clone(), false, ctx)
            }
        }
    }
}
impl View for ProjectsPageView {
    fn ui_name() -> &'static str {
        "ProjectsSettingsPage"
    }
    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}
impl SettingsPageMeta for ProjectsPageView {
    fn section() -> SettingsSection {
        SettingsSection::Projects
    }
    fn should_render(&self, _: &AppContext) -> bool {
        warp_core::channel::ChannelState::is_local_only()
    }
    fn on_page_selected(&mut self, _: bool, ctx: &mut ViewContext<Self>) {
        ProjectCatalog::handle(ctx).update(ctx, |catalog, ctx| catalog.refresh(ctx));
    }
    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }
    fn scroll_to_widget(&mut self, id: &'static str) {
        self.page.scroll_to_widget(id);
    }
    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}
impl From<ViewHandle<ProjectsPageView>> for SettingsPageViewHandle {
    fn from(handle: ViewHandle<ProjectsPageView>) -> Self {
        Self::Projects(handle)
    }
}
#[derive(Default)]
struct LocationsWidget<const PARENTS: bool> {
    add_state: MouseStateHandle,
}
fn copy(text: &str, appearance: &Appearance) -> Box<dyn Element> {
    Text::new_inline(text.to_owned(), appearance.ui_font_family(), 13.)
        .with_color(appearance.theme().foreground().into_solid())
        .with_clip(ClipConfig::ellipsis())
        .finish()
}
impl<const PARENTS: bool> SettingsWidget for LocationsWidget<PARENTS> {
    type View = ProjectsPageView;
    fn search_terms(&self) -> &str {
        if PARENTS {
            "projects parent folders discover subfolders directories"
        } else {
            "projects individual folders directories add remove"
        }
    }
    fn render(
        &self,
        view: &ProjectsPageView,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let (title, description, add_label) = if PARENTS {
            (
                "Discover projects in",
                "Each immediate subfolder becomes a project. Torch checks these folders when you open Cmd+P.",
                "Add parent folder",
            )
        } else {
            (
                "Project folders",
                "Choose a project in Cmd+P to resume its session, or start one if it is not open.",
                "Add project",
            )
        };
        let button = appearance
            .ui_builder()
            .button(ButtonVariant::Secondary, self.add_state.clone())
            .with_text_label(add_label.to_owned())
            .build()
            .on_click(|ctx, _, _| {
                ctx.dispatch_typed_action(ProjectsPageAction::Add { parent: PARENTS })
            })
            .finish();
        let label = render_body_item_label::<ProjectsPageAction>(
            title.into(),
            None,
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
        );
        let header = Flex::row()
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(
                Shrinkable::new(
                    1.,
                    Container::new(Align::new(label).left().finish()).finish(),
                )
                .finish(),
            )
            .with_child(
                Container::new(button)
                    .with_padding_right(TOGGLE_BUTTON_RIGHT_PADDING)
                    .finish(),
            )
            .finish();
        let description = appearance
            .ui_builder()
            .paragraph(description.to_owned())
            .with_style(UiComponentStyles {
                font_color: Some(appearance.theme().foreground().into_solid()),
                font_size: Some(12.),
                ..Default::default()
            })
            .build()
            .finish();
        let mut column = Flex::column().with_child(header).with_child(
            Container::new(description)
                .with_padding_top(4.)
                .with_margin_right(100.)
                .with_padding_bottom(HEADER_PADDING)
                .finish(),
        );
        if !PARENTS && let Some(error) = &view.error {
            column.add_child(copy(error, appearance));
        }
        let settings = ProjectsSettings::as_ref(app);
        let paths = if PARENTS {
            settings.parent_directories.value()
        } else {
            settings.directories.value()
        };
        if paths.is_empty() {
            column.add_child(
                Container::new(copy(
                    if PARENTS {
                        "No parent folders added."
                    } else {
                        "No projects yet. Add a folder to get started."
                    },
                    appearance,
                ))
                .with_padding_bottom(16.)
                .finish(),
            );
        }
        for path in paths {
            let state = view
                .remove_states
                .borrow_mut()
                .entry((PARENTS, path.clone()))
                .or_default()
                .clone();
            let remove_path = path.clone();
            let remove = appearance
                .ui_builder()
                .button(ButtonVariant::Text, state)
                .with_text_label("Remove".into())
                .build()
                .on_click(move |ctx, _, _| {
                    ctx.dispatch_typed_action(ProjectsPageAction::Remove {
                        parent: PARENTS,
                        path: remove_path.clone(),
                    })
                })
                .finish();
            let mut text = path.to_string_lossy().into_owned();
            if ProjectCatalog::as_ref(app).unavailable.contains(path) {
                text.push_str(" — unavailable");
            }
            column.add_child(
                Container::new(
                    Flex::row()
                        .with_cross_axis_alignment(CrossAxisAlignment::Center)
                        .with_spacing(16.)
                        .with_child(
                            Expanded::new(
                                1.,
                                Text::new_inline(text, appearance.ui_font_family(), 13.)
                                    .with_color(appearance.theme().foreground().into_solid())
                                    .with_clip(ClipConfig::start())
                                    .finish(),
                            )
                            .finish(),
                        )
                        .with_child(remove)
                        .finish(),
                )
                .with_padding_bottom(8.)
                .finish(),
            );
        }
        column.finish()
    }
}

#[cfg(test)]
#[path = "projects_page_tests.rs"]
mod tests;
