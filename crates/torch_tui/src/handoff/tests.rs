use torch::tui_export::{register_tui_session_view_test_singletons, slash_commands};
use torch_core::features::FeatureFlag;
use torch_editor::model::CoreEditorModel;
use torchui::platform::WindowStyle;
use torchui::{AddWindowOptions, App, ViewHandle};

use super::TuiTerminalSessionView;
use crate::autoupdate::TuiAutoupdater;
use crate::orchestration_model::TuiOrchestrationModel;
use crate::root_view::RootTuiView;
use crate::session_registry::TuiSessions;
use crate::test_fixtures::{add_test_semantic_selection, add_test_terminal_session};

struct Fixture {
    view: ViewHandle<TuiTerminalSessionView>,
}

fn fixture(app: &mut App) -> Fixture {
    register_tui_session_view_test_singletons(app);
    add_test_semantic_selection(app);
    app.update(TuiAutoupdater::register);
    app.update(crate::keybindings::init);
    let (window_id, _) = app.update(|ctx| {
        ctx.add_tui_window(
            AddWindowOptions {
                window_style: WindowStyle::NotStealFocus,
                ..Default::default()
            },
            RootTuiView::new,
        )
    });
    let sessions = app.add_singleton_model(|_| TuiSessions::new_for_test());
    let orchestration = app.update(TuiOrchestrationModel::register);
    app.update(|ctx| TuiSessions::wire_orchestration(&sessions, &orchestration, ctx));
    let (view, manager) = add_test_terminal_session(app, window_id);
    app.update(|ctx| {
        TuiSessions::register_session(&sessions, view.clone(), manager, true, ctx);
    });
    Fixture { view }
}

fn input_text(app: &App, fixture: &Fixture) -> String {
    fixture.view.read(app, |view, ctx| {
        view.input_view
            .as_ref(ctx)
            .model()
            .as_ref(ctx)
            .content()
            .as_ref(ctx)
            .text()
            .into_string()
    })
}

#[test]
fn slash_menu_selection_inserts_handoff_for_optional_prompt_composition() {
    let _oz_handoff = FeatureFlag::OzHandoff.override_enabled(true);
    let _local_cloud = FeatureFlag::HandoffLocalCloud.override_enabled(true);
    App::test((), |mut app| async move {
        let fixture = fixture(&mut app);
        fixture.view.update(&mut app, |view, ctx| {
            view.select_tui_slash_command(&slash_commands::MOVE_TO_CLOUD, ctx);
        });
        assert_eq!(input_text(&app, &fixture), "/handoff ");
    });
}
