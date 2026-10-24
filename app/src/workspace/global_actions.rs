use std::path::PathBuf;
use std::time::Duration;

use ::settings::ToggleableSetting;
use torch_core::execution_mode::AppExecutionMode;
use torch_errors::report_error;
use torchui::r#async::Timer;
use torchui::windowing::WindowManager;
use torchui::{AppContext, Entity, SingletonEntity, TypedActionView};

use crate::app_state::get_app_state;
use crate::network::NetworkStatus;
use crate::persistence::ModelEvent;
use crate::root_view::OpenPath;
use crate::terminal::alt_screen_reporting::AltScreenReporting;
use crate::terminal::general_settings::GeneralSettings;
use crate::undo_close::UndoCloseStack;
use crate::workspace::cross_window_tab_drag::CrossWindowTabDrag;
use crate::workspace::{Workspace, WorkspaceAction};
use crate::{GlobalResourceHandlesProvider, auth};

/// DEPRECATED. Global actions are being phased out.
/// Do not add any more global actions; use typed actions instead.
pub fn init_global_actions(app: &mut AppContext) {
    app.add_global_action("workspace:toggle_mouse_reporting", toggle_mouse_reporting);
    app.add_global_action("workspace:toggle_scroll_reporting", toggle_scroll_reporting);
    app.add_global_action("workspace:toggle_focus_reporting", toggle_focus_reporting);
    app.add_singleton_model(|_| PendingAppSave::default());
    app.add_global_action("workspace:save_app", save_app);
    app.add_global_action(
        "workspace:toggle_debug_network_status",
        toggle_debug_network_status,
    );
    app.add_global_action("workspace:open_repository", open_repository);
    app.add_global_action("app:undo_close", undo_close);
    app.add_global_action("app:log_out", trigger_log_out);
}

fn toggle_mouse_reporting(_: &(), ctx: &mut AppContext) {
    AltScreenReporting::handle(ctx).update(ctx, |reporting, ctx| {
        reporting
            .mouse_reporting_enabled
            .toggle_and_save_value(ctx)
            .expect("MouseReportingEnabled failed to serialize");
    });
}

fn toggle_scroll_reporting(_: &(), ctx: &mut AppContext) {
    AltScreenReporting::handle(ctx).update(ctx, |reporting, ctx| {
        reporting
            .scroll_reporting_enabled
            .toggle_and_save_value(ctx)
            .expect("ScrollReportingEnabled failed to serialize");
    });
}

fn toggle_focus_reporting(_: &(), ctx: &mut AppContext) {
    AltScreenReporting::handle(ctx).update(ctx, |reporting, ctx| {
        reporting
            .focus_reporting_enabled
            .toggle_and_save_value(ctx)
            .expect("FocusReportingEnabled failed to serialize");
    });
}

/// How long app-state save requests are gathered into one snapshot. Window moves, resizes, and
/// focus changes arrive in bursts, and each snapshot rebuilds and rewrites the whole app state.
const SAVE_APP_COALESCE_WINDOW: Duration = Duration::from_millis(750);

/// Whether an app-state snapshot is waiting for the end of its coalescing window.
#[derive(Default)]
pub struct PendingAppSave {
    scheduled: bool,
}

impl Entity for PendingAppSave {
    type Event = ();
}

impl SingletonEntity for PendingAppSave {}

fn save_app(_: &(), ctx: &mut AppContext) {
    PendingAppSave::handle(ctx).update(ctx, |pending, ctx| {
        if std::mem::replace(&mut pending.scheduled, true) {
            return;
        }
        ctx.spawn(Timer::after(SAVE_APP_COALESCE_WINDOW), |pending, _, ctx| {
            if std::mem::take(&mut pending.scheduled) {
                write_app_snapshot(ctx);
            }
        });
    });
}

/// Writes any app-state snapshot still waiting in its coalescing window.
pub fn flush_pending_save_app(ctx: &mut AppContext) {
    let scheduled = PendingAppSave::handle(ctx)
        .update(ctx, |pending, _| std::mem::take(&mut pending.scheduled));
    if scheduled {
        write_app_snapshot(ctx);
    }
}

/// Writes an app-state snapshot immediately, superseding any waiting one.
pub fn save_app_now(ctx: &mut AppContext) {
    PendingAppSave::handle(ctx).update(ctx, |pending, _| pending.scheduled = false);
    write_app_snapshot(ctx);
}

/// Doesn't touch [`PendingAppSave`], so its coalescing timer can call it while updating it.
fn write_app_snapshot(ctx: &AppContext) {
    if !AppExecutionMode::as_ref(ctx).can_save_session() {
        return;
    }

    if !*GeneralSettings::as_ref(ctx).restore_session {
        return;
    }

    // While a cross-window tab drag is active, the dragged tab's pane group
    // is in flight between source and preview windows and `get_app_state`
    // would produce a snapshot with zero windows. Persisting that snapshot
    // wipes the on-disk session via `save_app_state`'s delete-then-insert
    // transaction. `save_app` fires from window move / focus / resize /
    // close callbacks (see `app_callbacks` in `lib.rs`), all of which run
    // during a drag, so we have to short-circuit at this boundary. The
    // first save after the drag finalizes will rewrite the snapshot.
    if CrossWindowTabDrag::as_ref(ctx).is_active() {
        return;
    }

    let Some(model_event_sender) = GlobalResourceHandlesProvider::as_ref(ctx)
        .get()
        .model_event_sender
        .clone()
    else {
        return;
    };

    // Only compute the app state if we're definitely going to use it.
    let app_state = get_app_state(ctx);
    let event = ModelEvent::Snapshot(app_state);

    if let Err(err) = model_event_sender.send(event) {
        report_error!(anyhow::Error::new(err).context("Error trying to send model event"));
    }
}

fn toggle_debug_network_status(_: &(), ctx: &mut AppContext) {
    NetworkStatus::handle(ctx).update(ctx, move |me, ctx| {
        let is_reachable = me.is_online();
        let new_is_reachable = !is_reachable;
        if new_is_reachable {
            log::info!("Manually toggled network status to be reachable");
        } else {
            log::info!("Manually toggled network status to be not reachable");
        }
        me.reachability_changed(new_is_reachable, ctx)
    });
}

/// Reopens the last closed item (window or tab).
fn undo_close(_: &(), ctx: &mut AppContext) {
    UndoCloseStack::handle(ctx).update(ctx, |stack, ctx| {
        stack.undo_close(ctx);
    });
}

/// Dispatches an action to the active workspace, if one exists.
fn dispatch_to_active_workspace(ctx: &mut AppContext, action: WorkspaceAction) {
    if let Some(window_id) = WindowManager::as_ref(ctx).active_window()
        && let Some(workspaces) = ctx.views_of_type::<Workspace>(window_id)
        && let Some(workspace) = workspaces.into_iter().next()
    {
        workspace.update(ctx, |workspace, ctx| {
            workspace.handle_action(&action, ctx);
        });
    }
}

fn open_repository(path: &String, ctx: &mut AppContext) {
    if WindowManager::as_ref(ctx).active_window().is_some() {
        // There's an active window, dispatch to its workspace
        dispatch_to_active_workspace(
            ctx,
            WorkspaceAction::OpenRepository {
                path: Some(path.clone()),
            },
        );
    } else {
        // No active window, create a new one with the repository path
        let path_buf = PathBuf::from(path);
        ctx.dispatch_global_action("root_view:open_new_from_path", &OpenPath { path: path_buf });
    }
}

fn trigger_log_out(_: &(), ctx: &mut AppContext) {
    auth::log_out(ctx)
}

#[cfg(test)]
#[path = "global_actions_tests.rs"]
mod tests;
