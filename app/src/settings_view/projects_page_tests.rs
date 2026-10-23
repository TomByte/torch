use pathfinder_geometry::vector::vec2f;
use torchui::platform::WindowStyle;
use torchui::{App, Presenter, WindowInvalidation};

use super::*;
use crate::settings::{ProjectDirectories, ProjectParentDirectories};

#[test]
fn project_settings_layout_handles_empty_and_populated_lists() {
    for populated in [false, true] {
        App::test((), move |mut app| async move {
            app.add_singleton_model(|_| Appearance::mock());
            app.add_singleton_model(move |_| ProjectsSettings {
                directories: ProjectDirectories::new(Some(if populated {
                    vec![PathBuf::from("/projects/example")]
                } else {
                    Vec::new()
                })),
                parent_directories: ProjectParentDirectories::new(None),
            });
            app.add_singleton_model(|_| ProjectCatalog::default());
            let (window_id, _) = app.add_window(WindowStyle::NotStealFocus, ProjectsPageView::new);
            let root = app.root_view_id(window_id).unwrap();
            app.update(move |ctx| {
                let mut presenter = Presenter::new(window_id);
                for width in [800., 330.] {
                    presenter.invalidate(
                        WindowInvalidation {
                            updated: [root].into_iter().collect(),
                            ..Default::default()
                        },
                        ctx,
                    );
                    presenter.build_scene(vec2f(width, 600.), 1., None, ctx);
                }
            });
        });
    }
}

#[test]
fn project_settings_search_isolates_each_location_type() {
    use super::super::settings_page::FilteredPageType;
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = build_page();
            for (query, expected) in [("parent", 1), ("individual", 1), ("", 2)] {
                page.update_filter(query, ctx);
                let FilteredPageType::Categorized { categories, .. } = page.get_filtered() else {
                    panic!("expected project settings page");
                };
                let widgets: usize = categories
                    .iter()
                    .map(|category| category.widgets.len())
                    .sum();
                assert_eq!(widgets, expected);
            }
        });
    });
}
