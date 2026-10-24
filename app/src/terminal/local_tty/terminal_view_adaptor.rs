use std::any::Any;
use std::sync::Arc;
use std::sync::mpsc::SyncSender;

use parking_lot::FairMutex;
use torchui::{AppContext, ViewHandle, WindowId};

use super::terminal_manager::{TerminalManager, TerminalSurfaceInit, TerminalSurfaceResult};
use crate::context_chips::current_prompt::CurrentPrompt;
use crate::context_chips::prompt_type::PromptType;
use crate::pane_group::TerminalViewResources;
use crate::persistence::ModelEvent;
use crate::terminal::writeable_pty::terminal_manager_util::wire_up_remote_server_controller_with_view;
use crate::terminal::{TerminalManager as TerminalManagerTrait, TerminalModel, TerminalView};

/// Configuration for constructing the GUI terminal surface.
pub(crate) struct TerminalViewSurfaceConfig {
    pub(crate) resources: TerminalViewResources,
    pub(crate) model_event_sender: Option<SyncSender<ModelEvent>>,
    pub(crate) window_id: WindowId,
    pub(crate) has_restored_command_blocks: bool,
}

/// Creates the GUI terminal surface and its manager-owned post-wiring closure.
pub(crate) fn create_terminal_view_surface(
    config: TerminalViewSurfaceConfig,
    surface_init: TerminalSurfaceInit,
    ctx: &mut AppContext,
) -> TerminalSurfaceResult<
    TerminalView,
    impl FnOnce(&mut TerminalManager<TerminalView>, &ViewHandle<TerminalView>, &mut AppContext) + use<>,
> {
    let TerminalSurfaceInit {
        wakeups_rx,
        model_events,
        model,
        sessions,
        size_info,
        colors,
        inactive_pty_reads_rx,
    } = surface_init;
    let TerminalViewSurfaceConfig {
        resources,
        model_event_sender,
        window_id,
        has_restored_command_blocks,
    } = config;
    let current_prompt = ctx.add_model(|ctx| {
        CurrentPrompt::new_with_model_events(
            sessions.clone(),
            Some((&model_events, model.clone())),
            ctx,
        )
    });
    let prompt_type = ctx.add_model(|ctx| PromptType::new_dynamic(current_prompt.clone(), ctx));
    let view = ctx.add_typed_action_view(window_id, |ctx| {
        TerminalView::new(
            resources,
            wakeups_rx,
            model_events,
            model,
            sessions,
            size_info,
            colors,
            model_event_sender,
            prompt_type.clone(),
            Some(inactive_pty_reads_rx),
            ctx,
        )
    });

    TerminalSurfaceResult {
        surface: view,
        post_wire: move |terminal_manager: &mut TerminalManager<TerminalView>,
                         view: &ViewHandle<TerminalView>,
                         ctx: &mut AppContext| {
            if has_restored_command_blocks {
                terminal_manager
                    .model()
                    .lock()
                    .block_list_mut()
                    .append_session_restoration_separator_to_block_list();
            }

            wire_up_remote_server_controller_with_view(
                &terminal_manager.remote_server_controller(),
                view,
                ctx,
            );
        },
    }
}

impl TerminalManager<TerminalView> {
    /// Returns the PTY process id, for integration tests.
    #[cfg(feature = "integration_tests")]
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }
}

impl TerminalManagerTrait for TerminalManager<TerminalView> {
    fn model(&self) -> Arc<FairMutex<TerminalModel>> {
        self.model.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Send a Shutdown event to each PTY's event loop and waits for the
/// event loop to terminate.
/// This is needed on Windows to ensure all OpenConsole processes are
/// cleaned up before the main thread exits.
#[cfg(windows)]
pub fn shutdown_all_pty_event_loops(ctx: &mut AppContext) {
    let terminal_managers: Vec<ModelHandle<Box<dyn TerminalManagerTrait>>> = ctx.models_of_type();
    terminal_managers.into_iter().for_each(|terminal_manager| {
        terminal_manager.update(ctx, |terminal_manager, _ctx| {
            if let Some(manager) = terminal_manager
                .as_any_mut()
                .downcast_mut::<TerminalManager<TerminalView>>()
            {
                manager.shutdown_event_loop();
            }
        })
    })
}
