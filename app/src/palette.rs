use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum PaletteMode {
    Command,
    Navigation,
    LaunchConfig,
    Files,
    Agents,
}

/// How the command palette was opened. Only origins that change how the palette behaves are
/// distinguished.
#[derive(Clone, Copy, Debug)]
pub enum PaletteSource {
    CtrlTab { shift_pressed_initially: bool },
    TitleBarSearchBar,
    Other,
}
