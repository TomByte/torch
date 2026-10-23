use super::*;
use crate::workspace::view::tests::initialize_app;

#[test]
fn a_burst_of_save_requests_waits_for_one_snapshot_until_flushed() {
    torchui::App::test((), |mut app| async move {
        initialize_app(&mut app);
        app.update(|ctx| {
            assert!(!PendingAppSave::as_ref(ctx).scheduled);
            save_app(&(), ctx);
            save_app(&(), ctx);
            assert!(PendingAppSave::as_ref(ctx).scheduled);

            flush_pending_save_app(ctx);
            assert!(!PendingAppSave::as_ref(ctx).scheduled);

            save_app(&(), ctx);
            save_app_now(ctx);
            assert!(
                !PendingAppSave::as_ref(ctx).scheduled,
                "an immediate save supersedes the waiting one"
            );
        });
    });
}

#[test]
fn a_waiting_save_is_written_when_its_window_ends() {
    torchui::App::test((), |mut app| async move {
        initialize_app(&mut app);
        app.update(|ctx| save_app(&(), ctx));

        Timer::after(SAVE_APP_COALESCE_WINDOW * 2).await;

        app.read(|ctx| assert!(!PendingAppSave::as_ref(ctx).scheduled));
    });
}
