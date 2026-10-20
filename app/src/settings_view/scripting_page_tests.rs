use super::*;
use crate::settings_view::settings_page::FilteredPageType;
use warpui::App;

#[test]
fn claude_search_isolates_setup_and_keeps_scripting_title() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page: PageType<ScriptingSettingsPageView> = PageType::new_uncategorized(
                vec![
                    Box::new(LocalControlModeWidget),
                    Box::new(ClaudeHooksWidget::default()),
                ],
                Some(PageTitle::new("Scripting")),
            );
            page.update_filter("claude", ctx);
            let FilteredPageType::Uncategorized { widgets, title, .. } = page.get_filtered() else {
                panic!("expected widget list");
            };
            assert_eq!(widgets.len(), 1);
            assert!(title.is_some());
            page.update_filter("", ctx);
            let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
                panic!("expected widget list");
            };
            assert_eq!(widgets.len(), 2);
        });
    });
}

#[cfg(unix)]
#[test]
fn claude_setup_does_not_require_disabled_control_settings() {
    App::test((), |mut app| async move {
        crate::test_util::terminal::initialize_app_for_terminal_view(&mut app);
        assert!(!FeatureFlag::WarpControlCli.is_enabled());
        let (_, page) = app.add_window(
            warpui::platform::WindowStyle::NotStealFocus,
            ScriptingSettingsPageView::new,
        );
        page.read(&app, |page, _| {
            let FilteredPageType::Uncategorized { widgets, .. } = page.page.get_filtered() else {
                panic!("expected widget list");
            };
            assert_eq!(widgets.len(), 1);
            assert!(widgets[0].search_terms().contains("claude"));
        });
    });
}
