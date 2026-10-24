use input_classifier::InputType;
use serde::{Deserialize, Serialize};
use torch_core::features::FeatureFlag;
use torchui::{AppContext, SingletonEntity};

use crate::settings::AISettings;

/// Configuration for the terminal pane's input.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputConfig {
    /// The type of the terminal input.
    pub input_type: InputType,

    /// If `true`, we will not attempt to auto-detect the best input type.
    pub is_locked: bool,
}

impl InputConfig {
    /// Create a sensible default InputConfig based on user's auto-detection setting.
    pub fn new(app: &AppContext) -> Self {
        let ai_settings = AISettings::as_ref(app);
        let is_autodetection_enabled = ai_settings.is_ai_autodetection_enabled(app);

        InputConfig {
            input_type: InputType::Shell,
            is_locked: !is_autodetection_enabled, // Locked if auto-detection disabled
        }
    }

    pub fn with_toggled_type(self) -> Self {
        let input_type = if self.input_type.is_ai() {
            InputType::Shell
        } else {
            InputType::AI
        };
        Self { input_type, ..self }
    }

    pub fn with_shell_type(self) -> Self {
        Self {
            input_type: InputType::Shell,
            ..self
        }
    }

    pub fn with_input_type(self, input_type: InputType) -> Self {
        Self { input_type, ..self }
    }

    pub fn unlocked_if_autodetection_enabled(
        self,
        is_in_fullscreen_agent_view: bool,
        app: &AppContext,
    ) -> Self {
        Self {
            is_locked: if !FeatureFlag::AgentView.is_enabled() || is_in_fullscreen_agent_view {
                !AISettings::as_ref(app).is_ai_autodetection_enabled(app)
            } else {
                !AISettings::as_ref(app).is_nld_in_terminal_enabled(app)
            },
            ..self
        }
    }

    pub fn locked(self) -> Self {
        Self {
            is_locked: true,
            ..self
        }
    }

    pub fn is_ai(&self) -> bool {
        self.input_type == InputType::AI
    }

    pub fn is_shell(&self) -> bool {
        self.input_type == InputType::Shell
    }
}
