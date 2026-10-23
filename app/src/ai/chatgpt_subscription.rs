use std::collections::HashMap;
use std::sync::Arc;

use ai::api_keys::{ApiKeyManager, ChatGPTConnectFailure, ChatGPTConnectionStatus};
use torchui::{Entity, ModelContext, SingletonEntity};
use url::Url;

use crate::auth::AuthStateProvider;
use crate::server::server_api::ai::AIClient;

/// Host of the `{scheme}://chatgpt_link` deep link the browser opens once a link flow finishes.
pub const CHATGPT_LINK_URI_HOST: &str = "chatgpt_link";
const CHATGPT_LINK_STATE_PARAM: &str = "state";
const CHATGPT_LINKED_PARAM: &str = "chatgpt_linked";
const CHATGPT_ERROR_PARAM: &str = "chatgpt_error";

/// Keeps [`ApiKeyManager`]'s ChatGPT connection state in sync with warp-server,
/// which owns the account link and any delegated credentials.
pub struct ChatGPTSubscriptionModel {
    ai_client: Arc<dyn AIClient>,
    /// The `state` of the link attempt whose deep-link result is awaited, if any.
    pending_link_state: Option<String>,
}

impl ChatGPTSubscriptionModel {
    pub fn new(ai_client: Arc<dyn AIClient>) -> Self {
        Self {
            ai_client,
            pending_link_state: None,
        }
    }

    /// Settles the awaited link attempt from the deep link the browser flow opened.
    pub fn handle_link_redirect(&mut self, url: &Url, ctx: &mut ModelContext<Self>) {
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        let state = query.get(CHATGPT_LINK_STATE_PARAM);
        if self.pending_link_state.is_none() || state != self.pending_link_state.as_ref() {
            log::warn!("Ignoring ChatGPT link redirect that does not match a pending attempt");
            return;
        }
        self.pending_link_state = None;

        if let Some(error) = query.get(CHATGPT_ERROR_PARAM) {
            let failure = ChatGPTConnectFailure::from_deep_link_code(error);
            ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                manager.fail_chatgpt_oauth(failure, ctx);
            });
            return;
        }
        if !query.contains_key(CHATGPT_LINKED_PARAM) {
            ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                manager.fail_chatgpt_oauth(ChatGPTConnectFailure::Unknown, ctx);
            });
            return;
        }
        self.resolve_pending_link(ctx);
    }

    /// Re-fetches the connection status. Leaves any in-flight connect attempt
    /// untouched, since the user may still be in the browser.
    pub fn refresh(&mut self, ctx: &mut ModelContext<Self>) {
        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            return;
        }
        let ai_client = self.ai_client.clone();
        ctx.spawn(
            async move { ai_client.get_chatgpt_connection().await },
            |_, result, ctx| match result {
                Ok(status) => ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.set_chatgpt_connection_status(status, ctx);
                }),
                Err(e) => log::warn!("Failed to refresh ChatGPT connection: {e:#}"),
            },
        );
    }

    /// Re-fetches the connection status and uses the result to settle the connect attempt
    /// started from this client.
    fn resolve_pending_link(&mut self, ctx: &mut ModelContext<Self>) {
        if !AuthStateProvider::as_ref(ctx).get().is_logged_in() {
            return;
        }
        let ai_client = self.ai_client.clone();
        ctx.spawn(
            async move { ai_client.get_chatgpt_connection().await },
            |_, result, ctx| {
                let status = match result {
                    Ok(status) => Some(status),
                    Err(e) => {
                        log::warn!("Failed to refresh ChatGPT connection after linking: {e:#}");
                        None
                    }
                };
                ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
                    manager.resolve_chatgpt_oauth(status, ctx);
                });
            },
        );
    }

    /// Forgets the cached status, e.g. on logout.
    pub fn reset(&mut self, ctx: &mut ModelContext<Self>) {
        self.pending_link_state = None;
        ApiKeyManager::handle(ctx).update(ctx, |manager, ctx| {
            manager.set_chatgpt_connection_status(ChatGPTConnectionStatus::Unknown, ctx);
            manager.set_chatgpt_oauth_pending(false, ctx);
        });
    }
}

impl Entity for ChatGPTSubscriptionModel {
    type Event = ();
}

impl SingletonEntity for ChatGPTSubscriptionModel {}
