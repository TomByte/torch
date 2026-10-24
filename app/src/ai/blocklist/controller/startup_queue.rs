use torchui::{ModelContext, SingletonEntity};

use super::BlocklistAIController;
use crate::ai::agent::conversation::AIConversationId;
use crate::ai::blocklist::orchestration_events::OrchestrationEventService;
use crate::ai::blocklist::{
    BlocklistAIHistoryModel, QueuedPromptDeliveryMode, QueuedQueryId, QueuedQueryModel,
};

impl BlocklistAIController {
    /// Binds this controller to a native conversation before session sharing can begin
    /// delivering startup follow-ups, so `route_native_startup_injection` knows which
    /// conversation to target for the rest of this run. Idempotent: returns the existing
    /// binding if one is already in place.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub(crate) fn bind_native_prompt_conversation(
        &mut self,
        restored_conversation_id: Option<AIConversationId>,
        ctx: &mut ModelContext<Self>,
    ) -> AIConversationId {
        if let Some(id) = self.native_prompt_conversation_id {
            return id;
        }
        let id = restored_conversation_id.unwrap_or_else(|| {
            BlocklistAIHistoryModel::handle(ctx).update(ctx, |history, ctx| {
                history.start_new_conversation(self.terminal_surface_id, false, false, false, ctx)
            })
        });
        self.native_prompt_conversation_id = Some(id);
        log::info!(
            "event=native_queue_initialized task_id={:?} terminal_id={:?} conversation_id={id} resumed={}",
            self.ambient_agent_task_id,
            self.terminal_surface_id,
            restored_conversation_id.is_some(),
        );
        QueuedQueryModel::handle(ctx).update(ctx, |queue, ctx| {
            queue.begin_native_setup(id, ctx);
            queue.set_delivery_mode(id, QueuedPromptDeliveryMode::Steering);
        });
        id
    }

    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub(crate) fn native_prompt_conversation_id(&self) -> Option<AIConversationId> {
        self.native_prompt_conversation_id
    }

    /// Unbinds this controller from its native conversation, dropping any prompts still queued
    /// for it (e.g. the run ended before setup finished, or before a dispatch that was deferred
    /// behind an active CLI subagent could go out) and releasing the native setup barrier if it
    /// was still held -- otherwise this conversation would stay permanently dispatch-blocked for
    /// any future local queueing against it, since nothing else would ever release that barrier.
    ///
    /// For a conversation whose turns an external harness authored, any orchestration events
    /// still held behind that barrier are dropped first: releasing the barrier re-checks them,
    /// and injecting them would open a native request the server cannot serve.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    pub(crate) fn unbind_native_prompt_conversation(&mut self, ctx: &mut ModelContext<Self>) {
        let Some(id) = self.native_prompt_conversation_id.take() else {
            return;
        };
        self.external_harness_prompt_sink = None;
        self.external_harness_cancel_sink = None;
        let unsent_count = QueuedQueryModel::as_ref(ctx).queue(id).len();
        log::info!(
            "event=native_queue_stopped task_id={:?} terminal_id={:?} conversation_id={id} unsent_count={unsent_count}",
            self.ambient_agent_task_id,
            self.terminal_surface_id,
        );
        let owns_turn_lifecycle = BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&id)
            .is_none_or(|conversation| conversation.driver().owns_turn_lifecycle());
        if !owns_turn_lifecycle {
            OrchestrationEventService::handle(ctx).update(ctx, |service, _| {
                service.drop_pending_events_for_external_harness_conversation(id);
            });
        }
        QueuedQueryModel::handle(ctx).update(ctx, |queue, ctx| {
            queue.clear_queue(id, ctx);
            queue.finish_native_setup(id, ctx);
        });
    }

    /// Sends a prepared queued prompt. An explicit ID may interrupt; automatic sends preserve FIFO.
    pub(crate) fn dispatch_queued_warp_agent_prompt(
        &mut self,
        conversation_id: AIConversationId,
        query_id: Option<QueuedQueryId>,
        ctx: &mut ModelContext<Self>,
    ) {
        let has_active_subagent = BlocklistAIHistoryModel::as_ref(ctx)
            .conversation(&conversation_id)
            .is_some_and(|conversation| conversation.has_active_subagent());
        if has_active_subagent {
            log::info!(
                "event=native_queue_drain_deferred conversation_id={conversation_id} reason=active_subagent",
            );
            return;
        }
        let queue = QueuedQueryModel::as_ref(ctx);
        let row = match query_id {
            Some(query_id) => queue.ready_query(conversation_id, query_id),
            None => queue.ready_head(conversation_id),
        }
        .filter(|row| !row.is_command())
        .cloned();
        let Some(row) = row else {
            return;
        };
        let row_id = row.id();
        log::info!(
            "event=native_queue_row_dispatched task_id={:?} terminal_id={:?} conversation_id={conversation_id} query_id={row_id:?}",
            self.ambient_agent_task_id,
            self.terminal_surface_id,
        );
        // The send path resolves this row's attachments by id, so it must still be in the
        // queue when this is called; remove it only afterward.
        self.send_queued_user_query_in_conversation(
            row.text().to_owned(),
            conversation_id,
            row_id,
            ctx,
        );
        QueuedQueryModel::handle(ctx).update(ctx, |queue, ctx| {
            queue.remove_fired_row(conversation_id, row_id, ctx);
        });
    }
}
