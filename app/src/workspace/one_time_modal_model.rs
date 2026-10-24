use ai::api_keys::{ApiKeyManager, ApiKeyManagerEvent};
use settings::Setting as _;
use torch_core::features::FeatureFlag;
use torch_core::send_telemetry_from_ctx;
use torchui::{AppContext, Entity, ModelContext, SingletonEntity, WindowId};

use super::view::chatgpt_plan_modal::ChatGPTPlanModalTelemetryEvent;
use super::view::feature_intro_modal::{FEATURE_INTROS, FeatureIntroId};
use crate::auth::auth_manager::AuthManagerEvent;
use crate::auth::{AuthManager, AuthStateProvider, UserUid};
use crate::channel::{Channel, ChannelState};
use crate::root_view::RootView;
use crate::settings::cloud_preferences_syncer::{
    CloudPreferencesSyncer, CloudPreferencesSyncerEvent,
};
use crate::settings::{AISettings, CodeSettings};
use crate::terminal::general_settings::GeneralSettings;

/// A generic model for managing one-time modals that should be shown to users only once.
///
/// Initially implemented for the ADE launch modal, but designed to be extensible to support
/// other types of one-time modals in the future. The model holds the canonical state of whether
/// a modal is currently being shown and automatically triggers the modal when appropriate
/// conditions are met.
pub struct OneTimeModalModel {
    /// Whether the OpenWarp launch modal is currently being shown.
    is_openwarp_launch_modal_open: bool,
    /// Whether the Torch Agent CLI launch modal is currently being shown.
    is_agent_cli_launch_modal_open: bool,
    /// Whether the "You're using your ChatGPT plan" modal is currently being shown.
    is_chatgpt_plan_modal_open: bool,
    /// The feature-intro popover currently being shown, if any. Unlike the other
    /// one-time modals this is a non-blocking bottom-right popover, so it is
    /// intentionally excluded from `is_any_modal_open` (which suppresses terminal
    /// focus stealing) to keep the terminal usable while it is visible.
    active_feature_intro: Option<FeatureIntroId>,
    /// Whether the initial one-time modal checks have run. The seen markers are
    /// cloud-synced settings, so event-driven re-checks must wait for the initial
    /// cloud preferences load to avoid acting on stale values.
    has_completed_initial_modal_checks: bool,
    /// The user whose cloud preferences have completed an initial load this session. A
    /// re-login of the same user (e.g. the Sign in with ChatGPT handoff) resets the syncer's
    /// loaded state without necessarily re-emitting `InitialLoadCompleted`, but the synced
    /// markers already loaded for that user remain trustworthy. A full logout wipes the local
    /// copies of those markers, so it clears this too.
    cloud_preferences_loaded_for: Option<UserUid>,
    /// The window ID where the currently open one-time modal should be displayed.
    /// This is captured when a modal is first opened and ensures the modal stays on that window.
    target_window_id: Option<WindowId>,
}

impl OneTimeModalModel {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        if FeatureFlag::ChatGPTSubscription.is_enabled() {
            ctx.subscribe_to_model(
                &ApiKeyManager::handle(ctx),
                |me, _, event, ctx| match event {
                    ApiKeyManagerEvent::ChatGPTConnectionUpdated => {
                        me.maybe_trigger_chatgpt_plan_modal(ctx);
                    }
                    ApiKeyManagerEvent::KeysUpdated
                    | ApiKeyManagerEvent::ChatGPTConnectFailed(_) => {}
                },
            );
        }

        // Subscribe to auth manager events to automatically trigger modals when authentication completes
        ctx.subscribe_to_model(&AuthManager::handle(ctx), |_, _, event, ctx| {
            let AuthManagerEvent::AuthComplete = event else {
                return;
            };

            let auth_state = crate::auth::AuthStateProvider::as_ref(ctx).get().clone();
            let is_existing_user = auth_state.is_onboarded().unwrap_or_default();
            if is_existing_user {
                // Settings modals settings are synced to the cloud, not respecting the user's sync setting, so they
                // must all await initial load to be triggered, else we risk reading a stale triggered value.
                ctx.subscribe_to_model(
                    &CloudPreferencesSyncer::handle(ctx),
                    move |me, _, event, ctx| {
                        if let CloudPreferencesSyncerEvent::InitialLoadCompleted = event {
                            ctx.unsubscribe_from_model(&CloudPreferencesSyncer::handle(ctx));
                            me.record_cloud_preferences_loaded(ctx);
                            me.has_completed_initial_modal_checks = true;
                            me.check_and_trigger_all_modals(ctx);
                        }
                    },
                );
            } else {
                AISettings::handle(ctx).update(ctx, |settings, ctx| {
                    if let Err(e) = settings
                        .did_check_to_trigger_agent_cli_launch_modal
                        .set_value(true, ctx)
                    {
                        log::warn!("Failed to mark Torch Agent CLI launch modal as dismissed: {e}");
                    }
                    // New signups shouldn't see feature-intro popovers on their second
                    // startup, so pre-mark every registered feature intro as seen.
                    for intro in FEATURE_INTROS {
                        settings.mark_feature_intro_seen(intro.id.as_key(), ctx);
                    }
                });
                GeneralSettings::handle(ctx).update(ctx, |settings, ctx| {
                    if let Err(e) = settings
                        .did_check_to_trigger_openwarp_launch_modal
                        .set_value(true, ctx)
                    {
                        log::warn!("Failed to mark OpenWarp launch modal as dismissed: {e}");
                    }
                });
                // New signups skip the launch-modal queue, but the ChatGPT plan modal is
                // aimed at them and its seen marker is cloud-synced like the others, so it
                // still waits for the initial preferences load.
                if FeatureFlag::ChatGPTSubscription.is_enabled() {
                    ctx.subscribe_to_model(
                        &CloudPreferencesSyncer::handle(ctx),
                        move |me, _, event, ctx| {
                            if let CloudPreferencesSyncerEvent::InitialLoadCompleted = event {
                                ctx.unsubscribe_from_model(&CloudPreferencesSyncer::handle(ctx));
                                me.record_cloud_preferences_loaded(ctx);
                                me.maybe_trigger_chatgpt_plan_modal(ctx);
                            }
                        },
                    );
                }
            }
        });

        Self {
            is_openwarp_launch_modal_open: false,
            is_agent_cli_launch_modal_open: false,
            is_chatgpt_plan_modal_open: false,
            active_feature_intro: None,
            has_completed_initial_modal_checks: false,
            cloud_preferences_loaded_for: None,
            target_window_id: None,
        }
    }

    fn record_cloud_preferences_loaded(&mut self, ctx: &ModelContext<Self>) {
        self.cloud_preferences_loaded_for = AuthStateProvider::as_ref(ctx).get().user_id();
    }

    /// Forgets that the current user's cloud preferences were loaded. Logout clears the local
    /// copies of the synced markers, so the next login must wait for a fresh initial load
    /// before trusting them again.
    pub fn on_log_out(&mut self) {
        self.cloud_preferences_loaded_for = None;
    }

    /// Whether the current user's cloud-synced markers can be trusted: either the syncer is in
    /// its loaded state, or it completed a load for this same user earlier in the session.
    fn are_cloud_preferences_loaded(&self, ctx: &ModelContext<Self>) -> bool {
        if CloudPreferencesSyncer::as_ref(ctx).has_completed_initial_load() {
            return true;
        }
        self.cloud_preferences_loaded_for.is_some()
            && self.cloud_preferences_loaded_for == AuthStateProvider::as_ref(ctx).get().user_id()
    }

    /// Returns the window ID where the currently open one-time modal should be displayed.
    pub fn target_window_id(&self) -> Option<WindowId> {
        self.target_window_id
    }

    /// Returns whether the OpenWarp launch modal is currently open.
    pub fn is_openwarp_launch_modal_open(&self) -> bool {
        self.is_openwarp_launch_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_openwarp_launch_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_openwarp_launch_modal_open(false, ctx);
    }

    pub fn is_agent_cli_launch_modal_open(&self) -> bool {
        self.is_agent_cli_launch_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_agent_cli_launch_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_agent_cli_launch_modal_open(false, ctx);
    }

    /// Returns the feature-intro popover currently being shown, if any.
    pub fn active_feature_intro(&self) -> Option<FeatureIntroId> {
        if self.target_window_id.is_some() {
            self.active_feature_intro
        } else {
            None
        }
    }

    pub fn mark_feature_intro_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_active_feature_intro(None, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_feature_intro(&mut self, id: FeatureIntroId, ctx: &mut ModelContext<Self>) {
        self.set_active_feature_intro(Some(id), ctx);
    }

    fn set_active_feature_intro(
        &mut self,
        intro: Option<FeatureIntroId>,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.active_feature_intro != intro {
            self.active_feature_intro = intro;
            // Bind the popover to the focused window as soon as it opens. The
            // workspace only renders / populates the view when
            // `target_window_id` matches, and `on_active_window_changed` may not
            // have run yet when the startup modal queue fires.
            if intro.is_some()
                && self.target_window_id.is_none()
                && let Some(window_id) = ctx.windows().active_window()
            {
                self.target_window_id = Some(window_id);
            }
            ctx.emit(OneTimeModalEvent::VisibilityChanged {
                is_open: intro.is_some(),
            });
            return true;
        }
        false
    }

    /// Returns true if any one-time modal is currently open.
    pub fn is_any_modal_open(&self) -> bool {
        (self.is_openwarp_launch_modal_open
            || self.is_agent_cli_launch_modal_open
            || self.is_chatgpt_plan_modal_open)
            && self.target_window_id.is_some()
    }

    /// Returns whether the "You're using your ChatGPT plan" modal is currently open.
    pub fn is_chatgpt_plan_modal_open(&self) -> bool {
        self.is_chatgpt_plan_modal_open && self.target_window_id.is_some()
    }

    pub fn mark_chatgpt_plan_modal_dismissed(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_chatgpt_plan_modal_open(false, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_chatgpt_plan_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_chatgpt_plan_modal_open(true, ctx);
    }

    fn set_chatgpt_plan_modal_open(&mut self, is_open: bool, ctx: &mut ModelContext<Self>) -> bool {
        if self.is_chatgpt_plan_modal_open != is_open {
            self.is_chatgpt_plan_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    /// Event-driven re-check that first confirms the target window is a workspace window.
    /// Returns true when the modal was opened.
    fn maybe_trigger_chatgpt_plan_modal(&mut self, ctx: &mut ModelContext<Self>) -> bool {
        let Some(window_id) = self
            .target_window_id
            .or_else(|| ctx.windows().active_window())
        else {
            return false;
        };
        if !is_workspace_shown_in_window(window_id, ctx) {
            return false;
        }
        self.check_and_trigger_chatgpt_plan_modal(window_id, ctx)
    }

    /// Shows the ChatGPT plan modal once per connection: the subscription must be active, the
    /// cloud-synced seen marker must be loaded and unset, and no other one-time modal may be
    /// open. Returns true when the modal was opened.
    fn check_and_trigger_chatgpt_plan_modal(
        &mut self,
        window_id: WindowId,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if cfg!(target_family = "wasm") || !FeatureFlag::ChatGPTSubscription.is_enabled() {
            return false;
        }
        if self.is_chatgpt_plan_modal_open
            || self.is_any_modal_open()
            || self.active_feature_intro.is_some()
        {
            return false;
        }
        if !ApiKeyManager::as_ref(ctx).has_chatgpt_subscription() {
            return false;
        }
        if !self.are_cloud_preferences_loaded(ctx)
            || *AISettings::as_ref(ctx).did_show_chatgpt_plan_modal
        {
            return false;
        }

        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            if let Err(e) = settings.did_show_chatgpt_plan_modal.set_value(true, ctx) {
                log::warn!("Failed to mark ChatGPT plan modal as shown: {e}");
            }
        });

        let should_show = !matches!(ChannelState::channel(), Channel::Integration);
        if should_show {
            send_telemetry_from_ctx!(ChatGPTPlanModalTelemetryEvent::Shown, ctx);
            self.target_window_id = Some(window_id);
        }
        self.set_chatgpt_plan_modal_open(should_show, ctx);
        should_show
    }

    #[cfg(debug_assertions)]
    pub fn force_open_openwarp_launch_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_openwarp_launch_modal_open(true, ctx);
    }

    #[cfg(debug_assertions)]
    pub fn force_open_agent_cli_launch_modal(&mut self, ctx: &mut ModelContext<Self>) {
        self.set_agent_cli_launch_modal_open(true, ctx);
    }

    pub fn update_target_window_id(&mut self, window_id: WindowId, ctx: &mut ModelContext<Self>) {
        let was_any_modal_visible = self.is_any_modal_open();
        // Feature intro is intentionally excluded from `is_any_modal_open`, so
        // track it separately. Without this, activating a window after the
        // startup queue already selected an intro never re-emits, and the
        // workspace never calls `show_feature_intro_modal`.
        let was_feature_intro_visible = self.active_feature_intro().is_some();
        let previous_target = self.target_window_id;
        self.target_window_id = Some(window_id);
        let is_any_modal_visible = self.is_any_modal_open();
        let is_feature_intro_visible = self.active_feature_intro().is_some();
        if was_any_modal_visible != is_any_modal_visible
            || was_feature_intro_visible != is_feature_intro_visible
            || (is_feature_intro_visible && previous_target != Some(window_id))
        {
            ctx.emit(OneTimeModalEvent::VisibilityChanged {
                is_open: is_any_modal_visible || is_feature_intro_visible,
            });
        }
    }

    fn set_openwarp_launch_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_openwarp_launch_modal_open != is_open {
            self.is_openwarp_launch_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    fn set_agent_cli_launch_modal_open(
        &mut self,
        is_open: bool,
        ctx: &mut ModelContext<Self>,
    ) -> bool {
        if self.is_agent_cli_launch_modal_open != is_open {
            self.is_agent_cli_launch_modal_open = is_open;
            ctx.emit(OneTimeModalEvent::VisibilityChanged { is_open });
            return true;
        }
        false
    }

    fn check_and_trigger_all_modals(&mut self, ctx: &mut ModelContext<Self>) {
        // Never show one-time modals on WASM.
        if cfg!(target_family = "wasm") {
            return;
        }

        // Existing users should never see the code toolbelt new feature popup.
        CodeSettings::handle(ctx).update(ctx, |settings, ctx| {
            if let Err(e) = settings
                .dismissed_code_toolbelt_new_feature_popup
                .set_value(true, ctx)
            {
                log::warn!("Failed to mark code toolbelt new feature popup as dismissed: {e}");
            }
        });

        // The ChatGPT plan modal may already have opened from a connection-status event;
        // either way, don't stack launch modals on top of it. They keep their unseen
        // markers and show on a later startup.
        if self.is_chatgpt_plan_modal_open || self.maybe_trigger_chatgpt_plan_modal(ctx) {
            return;
        }

        if self.check_and_trigger_openwarp_launch_modal(ctx) {
            return;
        }

        if self.check_and_trigger_agent_cli_launch_modal(ctx) {
            return;
        }

        if self.check_and_trigger_feature_intro_modal(ctx) {
            return;
        }
    }

    fn check_and_trigger_openwarp_launch_modal(&mut self, ctx: &mut ModelContext<Self>) -> bool {
        // Only show if the feature flag is enabled.
        if !FeatureFlag::OpenWarpLaunchModal.is_enabled() {
            return false;
        }

        let general_settings = GeneralSettings::as_ref(ctx);
        let openwarp_modal_shown = *general_settings
            .did_check_to_trigger_openwarp_launch_modal
            .value();

        if openwarp_modal_shown {
            return false;
        }

        GeneralSettings::handle(ctx).update(ctx, |settings, ctx| {
            if let Err(e) = settings
                .did_check_to_trigger_openwarp_launch_modal
                .set_value(true, ctx)
            {
                log::warn!("Failed to mark OpenWarp launch modal as dismissed: {e}");
            }
        });

        let should_show_openwarp_modal = !matches!(ChannelState::channel(), Channel::Integration);
        self.set_openwarp_launch_modal_open(should_show_openwarp_modal, ctx);
        should_show_openwarp_modal
    }

    fn check_and_trigger_agent_cli_launch_modal(&mut self, ctx: &mut ModelContext<Self>) -> bool {
        if !FeatureFlag::AgentCliLaunchModal.is_enabled() {
            return false;
        }

        let ai_settings = AISettings::as_ref(ctx);
        if *ai_settings.did_check_to_trigger_agent_cli_launch_modal {
            return false;
        }

        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            if let Err(e) = settings
                .did_check_to_trigger_agent_cli_launch_modal
                .set_value(true, ctx)
            {
                log::warn!("Failed to mark Torch Agent CLI launch modal as dismissed: {e}");
            }
        });

        let should_show = !matches!(ChannelState::channel(), Channel::Integration);
        self.set_agent_cli_launch_modal_open(should_show, ctx);
        should_show
    }

    fn check_and_trigger_feature_intro_modal(&mut self, ctx: &mut ModelContext<Self>) -> bool {
        if !AISettings::as_ref(ctx).is_any_ai_enabled(ctx) {
            return false;
        }
        // Show the first registered feature intro that the user hasn't seen yet
        // (see `FEATURE_INTROS`).
        let next_id = FEATURE_INTROS
            .iter()
            .find(|intro| !AISettings::as_ref(ctx).is_feature_intro_seen(intro.id.as_key()))
            .map(|intro| intro.id);
        let Some(id) = next_id else {
            return false;
        };

        // Mark it seen up front so it shows at most once, even if suppressed below.
        AISettings::handle(ctx).update(ctx, |settings, ctx| {
            settings.mark_feature_intro_seen(id.as_key(), ctx);
        });

        let should_show = !matches!(ChannelState::channel(), Channel::Integration);
        if should_show {
            self.set_active_feature_intro(Some(id), ctx);
        }
        should_show
    }
}

/// Whether `window_id` is a workspace window.
fn is_workspace_shown_in_window(window_id: WindowId, app: &AppContext) -> bool {
    app.views_of_type::<RootView>(window_id)
        .is_some_and(|root_views| !root_views.is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OneTimeModalEvent {
    VisibilityChanged { is_open: bool },
}

impl Entity for OneTimeModalModel {
    type Event = OneTimeModalEvent;
}

impl SingletonEntity for OneTimeModalModel {}
