pub mod docker_sandbox;
pub mod terminal_manager;
mod terminal_view_adaptor;

pub use terminal_manager::TerminalManager;
#[cfg(windows)]
pub use terminal_view_adaptor::shutdown_all_pty_event_loops;
#[cfg(feature = "local_tty")]
pub(crate) use terminal_view_adaptor::{TerminalViewSurfaceConfig, create_terminal_view_surface};
pub use torch_terminal::local_tty::*;

#[cfg(unix)]
pub fn run_terminal_server(args: &torch_cli::TerminalServerArgs) {
    torch_terminal::local_tty::server::run_terminal_server(
        args,
        crate::features::init_feature_flags,
        crate::terminal::platform::init,
    );
}

impl event_loop::ActiveTerminal for crate::terminal::TerminalModel {
    fn exit(&mut self, reason: crate::terminal::model::terminal_model::ExitReason) {
        crate::terminal::TerminalModel::exit(self, reason);
    }
}

#[cfg(test)]
#[path = "event_loop_tests.rs"]
mod event_loop_tests;
