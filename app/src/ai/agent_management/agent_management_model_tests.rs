use settings::Setting as _;
use torch_core::features::FeatureFlag;
use torch_errors::report_if_error;
use torchui::{App, EntityId, ModelHandle, SingletonEntity};

use super::AgentNotificationsModel;
use crate::BlocklistAIHistoryModel;
use crate::ai::active_agent_views_model::ActiveAgentViewsModel;
use crate::ai::agent::conversation::{AIConversationId, ConversationStatus};
use crate::ai::agent_management::notifications::{
    NotificationCategory, NotificationFilter, NotificationOrigin, NotificationSourceAgent,
};
use crate::ai::artifacts::Artifact;
use crate::ai::blocklist::BlocklistAIHistoryEvent;
use crate::settings::AISettings;
use crate::terminal::cli_agent_sessions::CLIAgentSessionsModel;
use crate::test_util::settings::initialize_settings_for_tests;

fn setup_app(
    app: &mut App,
) -> (
    ModelHandle<BlocklistAIHistoryModel>,
    ModelHandle<AgentNotificationsModel>,
) {
    initialize_settings_for_tests(app);
    let history = app.add_singleton_model(|_| BlocklistAIHistoryModel::new(vec![], vec![], &[]));
    // Registered after the history model since it subscribes to history events; the
    // notifications model reads it to suppress completion notifications when a prompt is queued.
    app.add_singleton_model(crate::ai::blocklist::QueuedQueryModel::new);
    app.add_singleton_model(|_| CLIAgentSessionsModel::new());
    app.add_singleton_model(|_| ActiveAgentViewsModel::new());
    let notifications = app.add_singleton_model(AgentNotificationsModel::new);
    (history, notifications)
}

fn make_pr_artifact(url: &str, branch: &str) -> Artifact {
    Artifact::PullRequest {
        url: url.to_string(),
        branch: branch.to_string(),
        repo: None,
        number: None,
    }
}

#[cfg(feature = "local_only")]
#[test]
fn local_cli_completion_can_be_read_without_warp_notifications_enabled() {
    use crate::terminal::CLIAgent;
    use crate::terminal::cli_agent_sessions::{
        CLIAgentSessionContext, CLIAgentSessionStatus, CLIAgentSessionsModelEvent,
    };

    App::test((), |mut app| async move {
        let _guard = FeatureFlag::HOANotifications.override_enabled(false);
        let (_history, notifications) = setup_app(&mut app);
        app.add_singleton_model(|_| crate::workspace::WorkspaceRegistry::new());
        let terminal_view_id = EntityId::new();
        notifications.update(&mut app, |model, ctx| {
            model.handle_cli_agent_session_event(
                &CLIAgentSessionsModelEvent::StatusChanged {
                    terminal_view_id,
                    agent: CLIAgent::Claude,
                    status: CLIAgentSessionStatus::Success,
                    is_prompt_submit: false,
                    session_context: Box::new(CLIAgentSessionContext::default()),
                },
                ctx,
            );
            assert!(
                model
                    .notifications()
                    .has_unread_for_terminal_view(terminal_view_id)
            );
            model.mark_items_from_terminal_view_read(terminal_view_id, ctx);
            assert!(
                !model
                    .notifications()
                    .has_unread_for_terminal_view(terminal_view_id)
            );
        });
    });
}

#[cfg(feature = "local_only")]
#[test]
fn local_cli_notifications_use_torch_wording_and_clear_when_the_agent_moves_on() {
    use crate::terminal::CLIAgent;
    use crate::terminal::cli_agent_sessions::{
        BlockedSource, CLIAgentSessionContext, CLIAgentSessionStatus, CLIAgentSessionsModelEvent,
    };

    App::test((), |mut app| async move {
        let (_history, notifications) = setup_app(&mut app);
        app.add_singleton_model(|_| crate::workspace::WorkspaceRegistry::new());
        let terminal_view_id = EntityId::new();
        let status_changed = |status| CLIAgentSessionsModelEvent::StatusChanged {
            terminal_view_id,
            agent: CLIAgent::Claude,
            status,
            is_prompt_submit: false,
            session_context: Box::new(CLIAgentSessionContext {
                tool_name: Some("Bash".to_owned()),
                ..Default::default()
            }),
        };
        notifications.update(&mut app, |model, ctx| {
            model.handle_cli_agent_session_event(
                &status_changed(CLIAgentSessionStatus::Blocked {
                    message: None,
                    source: BlockedSource::PermissionRequest,
                }),
                ctx,
            );
            let item = model
                .notifications()
                .items_filtered(NotificationFilter::All)
                .next()
                .expect("a blocked agent is notified");
            assert_eq!(item.category, NotificationCategory::Request);
            assert_eq!(item.message, "Claude Code wants to use Bash");

            for status in [
                CLIAgentSessionStatus::InProgress,
                CLIAgentSessionStatus::Cancelled,
            ] {
                model.handle_cli_agent_session_event(&status_changed(status), ctx);
                assert_eq!(
                    model
                        .notifications()
                        .filtered_count(NotificationFilter::All),
                    0
                );
            }
        });
    });
}

#[test]
fn add_notification_tracks_unread_activity_when_in_app_notifications_are_hidden() {
    App::test((), |mut app| async move {
        let _guard = FeatureFlag::HOANotifications.override_enabled(true);
        let (_history, notifications) = setup_app(&mut app);

        AISettings::handle(&app).update(&mut app, |settings, ctx| {
            report_if_error!(settings.show_agent_notifications.set_value(false, ctx));
        });

        let conversation_id = AIConversationId::new();
        let terminal_view_id = EntityId::new();
        notifications.update(&mut app, |model, ctx| {
            model.add_notification(
                "Agent task".to_owned(),
                "Task completed.".to_owned(),
                NotificationCategory::Complete,
                NotificationSourceAgent::Oz { is_ambient: false },
                NotificationOrigin::Conversation(conversation_id),
                terminal_view_id,
                vec![],
                None,
                ctx,
            );
        });

        notifications.read(&app, |model, _| {
            assert_eq!(
                model
                    .notifications()
                    .filtered_count(NotificationFilter::All),
                1
            );
            assert!(
                model
                    .notifications()
                    .has_unread_for_terminal_view(terminal_view_id)
            );
        });
    });
}

#[test]
fn flush_returns_empty_vec_when_no_artifacts() {
    App::test((), |mut app| async move {
        let _guard = FeatureFlag::HOANotifications.override_enabled(true);
        let (_history, notifications) = setup_app(&mut app);

        let conversation_id = AIConversationId::new();

        notifications.update(&mut app, |model, _| {
            let artifacts = model.flush_pending_artifacts(conversation_id);
            assert!(artifacts.is_empty());
        });
    });
}

#[test]
fn deletion_cleans_up_pending_artifacts() {
    App::test((), |mut app| async move {
        let _guard = FeatureFlag::HOANotifications.override_enabled(true);
        let (history, notifications) = setup_app(&mut app);

        let conversation_id = AIConversationId::new();
        let terminal_view_id = EntityId::new();

        history.update(&mut app, |_: &mut BlocklistAIHistoryModel, ctx| {
            ctx.emit(BlocklistAIHistoryEvent::UpdatedConversationArtifacts {
                terminal_surface_id: terminal_view_id,
                conversation_id,
                artifact: make_pr_artifact("https://github.com/org/repo/pull/1", "branch-1"),
            });
        });

        history.update(&mut app, |_: &mut BlocklistAIHistoryModel, ctx| {
            ctx.emit(BlocklistAIHistoryEvent::DeletedConversation {
                terminal_surface_id: terminal_view_id,
                conversation_id,
                conversation_title: None,
                run_id: None,
            });
        });

        notifications.read(&app, |model, _| {
            assert!(!model.pending_artifacts.contains_key(&conversation_id));
        });
    });
}

// should_trigger_notification: pure-function tests pinning which statuses
// fire user-facing notifications. Terminal-error and blocked surface;
// in-progress, waiting-for-events, and user-cancelled do not.

#[test]
fn should_trigger_notification_returns_true_for_success() {
    assert!(ConversationStatus::Success.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_true_for_blocked() {
    assert!(
        ConversationStatus::Blocked {
            blocked_action: "approve diff".to_owned(),
        }
        .should_trigger_notification()
    );
}

#[test]
fn should_trigger_notification_returns_true_for_error() {
    assert!(ConversationStatus::Error.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_false_for_in_progress() {
    assert!(!ConversationStatus::InProgress.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_false_for_waiting_for_events() {
    assert!(!ConversationStatus::WaitingForEvents.should_trigger_notification());
}

#[test]
fn should_trigger_notification_returns_false_for_cancelled() {
    assert!(!ConversationStatus::Cancelled.should_trigger_notification());
}

// Mailbox suppression for non-terminal status updates. In App::test the
// `is_conversation_open` gate always returns false, so the
// WaitingForEvents and InProgress arms both clear stale notifications
// regardless of status; this still pins the user-visible contract that
// no stale "Task completed" toast survives a non-terminal transition.
