//! Settings UI for local scripting and Warp control permissions.
use std::cell::RefCell;
use std::collections::HashMap;

use settings::Setting as _;
use warp_core::channel::ChannelState;
use warp_errors::report_if_error;
use warpui::elements::{ChildView, Element, MouseStateHandle};
use warpui::ui_components::button::ButtonVariant;
use warpui::ui_components::components::UiComponent;
use warpui::{AppContext, Entity, SingletonEntity, TypedActionView, View, ViewContext, ViewHandle};

use super::settings_page::{
    LocalOnlyIconState, MatchData, PageTitle, PageType, SettingsPageMeta, SettingsPageViewHandle,
    SettingsWidget, render_body_item,
};
use super::{SettingsSection, ToggleState};
use crate::appearance::Appearance;
use crate::features::FeatureFlag;
use crate::settings::{LocalControlMode, LocalControlModeSetting, LocalControlSettings};
#[cfg(target_os = "macos")]
use crate::view_components::DismissibleToast;
use crate::view_components::{Dropdown, DropdownItem};
#[cfg(target_os = "macos")]
use crate::workspace::{ToastStack, cli_install};

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptingSettingsPageAction {
    SetLocalControlMode(LocalControlMode),
    ConfigureClaude(bool),
    ChooseClaudeFolder,
    #[cfg(target_os = "macos")]
    InstallWarpControlCli,
}

pub struct ScriptingSettingsPageView {
    page: PageType<Self>,
    hook_config_dir: Option<std::path::PathBuf>,
    hook_message: Option<String>,
    hook_busy: bool,
    local_only_icon_tooltip_states: RefCell<HashMap<String, MouseStateHandle>>,
    local_control_mode_dropdown: ViewHandle<Dropdown<ScriptingSettingsPageAction>>,
    #[cfg(target_os = "macos")]
    warpctrl_installing: bool,
}

impl ScriptingSettingsPageView {
    pub fn new(ctx: &mut ViewContext<Self>) -> Self {
        let local_control_mode_dropdown = ctx.add_typed_action_view(|ctx| {
            let mut dropdown = Dropdown::new(ctx);
            dropdown.set_top_bar_max_width(360.);
            dropdown
        });
        if FeatureFlag::WarpControlCli.is_enabled() {
            Self::update_local_control_mode_dropdown(local_control_mode_dropdown.clone(), ctx);
            ctx.subscribe_to_model(&LocalControlSettings::handle(ctx), |view, _, _, ctx| {
                Self::update_local_control_mode_dropdown(
                    view.local_control_mode_dropdown.clone(),
                    ctx,
                );
                ctx.notify();
            });
        }

        let mut widgets: Vec<Box<dyn SettingsWidget<View = Self>>> = Vec::new();
        if FeatureFlag::WarpControlCli.is_enabled() {
            #[cfg(target_os = "macos")]
            widgets.push(Box::new(WarpControlCliInstallWidget::default()));
            widgets.push(Box::new(LocalControlModeWidget));
        }
        if cfg!(unix) && ChannelState::is_local_only() {
            widgets.push(Box::new(ClaudeHooksWidget::default()));
        }

        Self {
            hook_config_dir: crate::local_hooks::default_config_dir(),
            hook_message: None,
            hook_busy: false,
            page: PageType::new_uncategorized(widgets, Some(PageTitle::new("Scripting"))),
            local_only_icon_tooltip_states: RefCell::new(HashMap::new()),
            local_control_mode_dropdown,
            #[cfg(target_os = "macos")]
            warpctrl_installing: false,
        }
    }

    fn update_local_control_mode_dropdown(
        dropdown: ViewHandle<Dropdown<ScriptingSettingsPageAction>>,
        ctx: &mut ViewContext<Self>,
    ) {
        let current_mode = LocalControlSettings::as_ref(ctx).mode();
        dropdown.update(ctx, |dropdown, ctx| {
            dropdown.set_items(
                LocalControlMode::ALL
                    .into_iter()
                    .map(|mode| {
                        DropdownItem::new(
                            mode.as_dropdown_label(),
                            ScriptingSettingsPageAction::SetLocalControlMode(mode),
                        )
                    })
                    .collect(),
                ctx,
            );
            dropdown.set_selected_by_action(
                ScriptingSettingsPageAction::SetLocalControlMode(current_mode),
                ctx,
            );
        });
    }

    #[cfg(target_os = "macos")]
    fn install_warpctrl(&mut self, ctx: &mut ViewContext<Self>) {
        if self.warpctrl_installing || cli_install::is_warpctrl_installed() {
            return;
        }

        self.warpctrl_installing = true;
        ctx.notify();
        let window_id = ctx.window_id();
        ctx.spawn(
            async { cli_install::install_warpctrl() },
            move |view, result, ctx| {
                view.warpctrl_installing = false;
                match result {
                    Ok(()) => {
                        let command_name = ChannelState::channel().warpctrl_command_name();
                        let message = format!(
                            "Successfully installed the Torch Control CLI! You can now run '{command_name}' from the command line."
                        );
                        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                            toast_stack.add_ephemeral_toast(
                                DismissibleToast::success(message),
                                window_id,
                                ctx,
                            );
                        });
                    }
                    Err(error) => {
                        let message = format!("Failed to install Torch Control command: {error}");
                        log::warn!("{message}");
                        ToastStack::handle(ctx).update(ctx, |toast_stack, ctx| {
                            toast_stack.add_persistent_toast(
                                DismissibleToast::error(message),
                                window_id,
                                ctx,
                            );
                        });
                    }
                }
                ctx.notify();
            },
        );
    }
}

impl Entity for ScriptingSettingsPageView {
    type Event = ();
}

impl TypedActionView for ScriptingSettingsPageView {
    type Action = ScriptingSettingsPageAction;

    fn handle_action(&mut self, action: &Self::Action, ctx: &mut ViewContext<Self>) {
        match action {
            ScriptingSettingsPageAction::ChooseClaudeFolder => {
                ctx.open_file_picker(
                    move |result, ctx| {
                        if let Ok(paths) = result
                            && let Some(path) = paths.into_iter().next()
                            && let Some(handle) = ctx.handle().upgrade(ctx)
                        {
                            handle.update(ctx, |view, ctx| {
                                view.hook_config_dir = Some(path.into());
                                view.hook_message = None;
                                ctx.notify();
                            });
                        }
                    },
                    warpui::platform::FilePickerConfiguration::new().folders_only(),
                );
            }
            ScriptingSettingsPageAction::ConfigureClaude(install) => {
                if self.hook_busy {
                    return;
                }
                let Some(directory) = self.hook_config_dir.clone() else {
                    return;
                };
                let install = *install;
                self.hook_busy = true;
                self.hook_message = None;
                ctx.notify();
                ctx.spawn(
                    async move { crate::local_hooks::configure(&directory, install) },
                    move |view, result, ctx| {
                        view.hook_busy = false;
                        view.hook_message = Some(match result {
                            Ok(backup) => {
                                let mut message = if install {
                                    "Installed. Open a new terminal and start a new Claude session."
                                        .to_owned()
                                } else {
                                    "Removed Torch hooks. Restart Claude to apply.".to_owned()
                                };
                                if let Some(path) = backup {
                                    message.push_str(&format!(" Backup: {}", path.display()));
                                }
                                message
                            }
                            Err(error) => format!("Could not change hooks: {error}"),
                        });
                        ctx.notify();
                    },
                );
            }
            ScriptingSettingsPageAction::SetLocalControlMode(mode) => {
                LocalControlSettings::handle(ctx).update(ctx, |settings, ctx| {
                    report_if_error!(settings.local_control_mode.set_value(*mode, ctx));
                });
                ctx.notify();
            }
            #[cfg(target_os = "macos")]
            ScriptingSettingsPageAction::InstallWarpControlCli => self.install_warpctrl(ctx),
        }
    }
}

impl View for ScriptingSettingsPageView {
    fn ui_name() -> &'static str {
        "ScriptingSettingsPage"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        self.page.render(self, app)
    }
}

impl SettingsPageMeta for ScriptingSettingsPageView {
    fn section() -> SettingsSection {
        SettingsSection::Scripting
    }

    fn should_render(&self, _ctx: &AppContext) -> bool {
        cfg!(not(target_family = "wasm"))
            && (FeatureFlag::WarpControlCli.is_enabled()
                || (cfg!(unix) && ChannelState::is_local_only()))
    }

    fn update_filter(&mut self, query: &str, ctx: &mut ViewContext<Self>) -> MatchData {
        self.page.update_filter(query, ctx)
    }

    fn scroll_to_widget(&mut self, widget_id: &'static str) {
        self.page.scroll_to_widget(widget_id)
    }

    fn clear_highlighted_widget(&mut self) {
        self.page.clear_highlighted_widget();
    }
}

impl From<ViewHandle<ScriptingSettingsPageView>> for SettingsPageViewHandle {
    fn from(view_handle: ViewHandle<ScriptingSettingsPageView>) -> Self {
        SettingsPageViewHandle::Scripting(view_handle)
    }
}

#[cfg(target_os = "macos")]
#[derive(Default)]
struct WarpControlCliInstallWidget {
    install_button_mouse_state: MouseStateHandle,
}

#[cfg(target_os = "macos")]
impl SettingsWidget for WarpControlCliInstallWidget {
    type View = ScriptingSettingsPageView;

    fn search_terms(&self) -> &str {
        "warp control cli command warpctrl install scripting"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _app: &AppContext,
    ) -> Box<dyn Element> {
        let installed = cli_install::is_warpctrl_installed();
        let disabled = view.warpctrl_installing || installed;
        let label = if view.warpctrl_installing {
            "Installing…"
        } else if installed {
            "Installed"
        } else {
            "Install"
        };
        let mut button = appearance
            .ui_builder()
            .button(
                ButtonVariant::Secondary,
                self.install_button_mouse_state.clone(),
            )
            .with_text_label(label.to_owned());
        if disabled {
            button = button.disabled();
        }
        let button = if disabled {
            button.build().finish()
        } else {
            button
                .build()
                .on_click(|ctx, _, _| {
                    ctx.dispatch_typed_action(ScriptingSettingsPageAction::InstallWarpControlCli);
                })
                .finish()
        };

        render_body_item::<ScriptingSettingsPageAction>(
            "Torch Control CLI command".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            button,
            Some("Install the warpctrl command for scripting Torch from your terminal.".to_owned()),
        )
    }
}
struct LocalControlModeWidget;

impl SettingsWidget for LocalControlModeWidget {
    type View = ScriptingSettingsPageView;

    fn search_terms(&self) -> &str {
        "scripting warp control automation warpctrl local cli scripts disabled enabled"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        app: &AppContext,
    ) -> Box<dyn Element> {
        render_body_item::<ScriptingSettingsPageAction>(
            "warpctrl CLI".into(),
            None,
            LocalOnlyIconState::for_setting(
                LocalControlModeSetting::storage_key(),
                LocalControlModeSetting::sync_to_cloud(),
                &mut view.local_only_icon_tooltip_states.borrow_mut(),
                app,
            ),
            ToggleState::Enabled,
            appearance,
            ChildView::new(&view.local_control_mode_dropdown).finish(),
            Some("warpctrl allows for scripting Torch's UI. Use with care.".to_owned()),
        )
    }
}

#[derive(Default)]
struct ClaudeHooksWidget {
    install: MouseStateHandle,
    remove: MouseStateHandle,
    folder: MouseStateHandle,
}

impl SettingsWidget for ClaudeHooksWidget {
    type View = ScriptingSettingsPageView;

    fn search_terms(&self) -> &str {
        "claude code hooks session status working attention permission integration install remove scripting"
    }

    fn render(
        &self,
        view: &Self::View,
        appearance: &Appearance,
        _: &AppContext,
    ) -> Box<dyn Element> {
        use warpui::elements::{Flex, ParentElement};
        let mut buttons = Flex::row().with_spacing(8.);
        for (label, state, action) in [
            (
                "Install / repair",
                &self.install,
                ScriptingSettingsPageAction::ConfigureClaude(true),
            ),
            (
                "Remove",
                &self.remove,
                ScriptingSettingsPageAction::ConfigureClaude(false),
            ),
            (
                "Choose folder…",
                &self.folder,
                ScriptingSettingsPageAction::ChooseClaudeFolder,
            ),
        ] {
            let mut button = appearance
                .ui_builder()
                .button(ButtonVariant::Secondary, state.clone())
                .with_text_label(label.to_owned());
            if view.hook_busy {
                button = button.disabled();
            }
            buttons.add_child(
                button
                    .build()
                    .on_click(move |ctx, _, _| {
                        ctx.dispatch_typed_action(action.clone());
                    })
                    .finish(),
            );
        }
        let directory = view
            .hook_config_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "Choose your Claude configuration folder".to_owned());
        let mut description = format!(
            "Show when Claude is working or needs you. Requires a current Claude Code version. Existing hooks and permissions are preserved.\nClaude configuration: {directory}"
        );
        if let Some(message) = &view.hook_message {
            description.push_str(&format!("\n{message}"));
        }
        render_body_item::<ScriptingSettingsPageAction>(
            "Claude session status".into(),
            None,
            LocalOnlyIconState::Hidden,
            ToggleState::Enabled,
            appearance,
            buttons.finish(),
            Some(description),
        )
    }
}

#[cfg(test)]
#[path = "scripting_page_tests.rs"]
mod tests;
