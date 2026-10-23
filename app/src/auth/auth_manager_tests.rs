use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use torchui::{App, SingletonEntity};

use super::{AuthManager, request_device_code_with_timeout};
use crate::ServerApiProvider;
use crate::auth::credentials::Credentials;
use crate::auth::user::{FirebaseAuthTokens, TEST_USER_UID, User};
use crate::auth::{AuthStateProvider, UserUid};
use crate::server::server_api::auth::UserAuthenticationError;

fn initialize_app(app: &mut App) {
    app.add_singleton_model(|_ctx| ServerApiProvider::new_for_test());
    app.add_singleton_model(|_| AuthStateProvider::new_for_test());
    app.add_singleton_model(AuthManager::new_for_test);
}

#[test]
fn validated_api_key_is_promoted_with_its_user() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);
        let auth_state = app.read(|ctx| AuthStateProvider::as_ref(ctx).get().clone());
        auth_state.set_user(None);
        auth_state.set_credentials(None);

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            auth_manager.complete_authentication(
                User::test(),
                Credentials::ApiKey {
                    key: "wk-validated-key".to_owned(),
                    owner_type: None,
                },
                ctx,
            );
        });

        assert_eq!(auth_state.api_key().as_deref(), Some("wk-validated-key"));
        assert_eq!(auth_state.user_id(), Some(UserUid::new(TEST_USER_UID)));
        assert!(auth_state.is_logged_in());
    });
}

#[test]
fn test_device_code_request_retries_then_times_out() {
    App::test((), |_app| async move {
        let attempts = Arc::new(AtomicUsize::new(0));
        let attempts_for_request = attempts.clone();

        let result = request_device_code_with_timeout(
            move || {
                attempts_for_request.fetch_add(1, Ordering::Relaxed);
                futures::future::pending()
            },
            Duration::from_millis(1),
            2,
        )
        .await;

        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        let error = result.unwrap_err();
        assert!(matches!(
            &error,
            UserAuthenticationError::DeviceCodeRequestTimedOut { attempts: 2 }
        ));
        assert_eq!(
            error.to_string(),
            "Timed out requesting a sign-in link after 2 attempts"
        );
    });
}

/// `log_out` must clear any pending CSRF state from an auth flow that was
/// started but never completed, so the token cannot be replayed against the
/// next session in the same process.
#[test]
fn test_log_out_clears_pending_auth_state() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // `log_out` clears user+credentials and then calls `persist`, which
        // routes to `PersistedUser::remove_from_secure_storage`. That requires
        // a `SecureStorage` singleton, so register a no-op one for this test.
        app.update(|ctx| {
            torchui_extras::secure_storage::register_noop("warp_test", ctx);
        });

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            let _pending = auth_manager.generate_auth_state();
            assert!(
                auth_manager.pending_auth_state.is_some(),
                "precondition: generate_auth_state should populate pending_auth_state"
            );

            auth_manager.log_out(ctx);

            assert!(
                auth_manager.pending_auth_state.is_none(),
                "log_out should clear pending_auth_state"
            );
        });
    });
}

// These two tests verify that `persist` skips writing to secure storage under certain conditions.
// They rely on the fact that no secure storage singleton is registered in the test app: if
// `write_to_secure_storage` were ever called, it would panic trying to look up the unregistered
// singleton, causing the test to fail.

#[test]
fn test_persist_skips_when_refresh_token_is_empty() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        // Override default test credentials with Firebase tokens that have an empty refresh token.
        app.update(|ctx| {
            let tokens = FirebaseAuthTokens {
                id_token: String::new(),
                refresh_token: String::new(),
                expiration_time: chrono::Utc::now().fixed_offset() + chrono::Duration::days(365),
            };
            AuthStateProvider::as_ref(ctx)
                .get()
                .set_credentials(Some(Credentials::Firebase(tokens)));
        });

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            auth_manager.persist(ctx);
        });
    });
}

#[test]
fn test_persist_skips_when_api_key_authenticated() {
    App::test((), |mut app| async move {
        initialize_app(&mut app);

        app.update(|ctx| {
            AuthStateProvider::as_ref(ctx)
                .get()
                .set_credentials(Some(Credentials::ApiKey {
                    key: "wk-test-key".to_owned(),
                    owner_type: None,
                }));
        });

        AuthManager::handle(&app).update(&mut app, |auth_manager, ctx| {
            auth_manager.persist(ctx);
        });
    });
}
