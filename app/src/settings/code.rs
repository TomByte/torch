use settings::SupportedPlatforms;
use settings::macros::define_settings_group;

define_settings_group!(CodeSettings, settings: [
    code_as_default_editor: CodeAsDefaultEditor {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.use_warp_as_default_editor",
        description: "Whether Torch is used as the default code editor.",
    }
    // Whether or not the user has manually dismissed the code toolbelt new feature popup.
    dismissed_code_toolbelt_new_feature_popup: DismissedCodeToolbeltNewFeaturePopup {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    // Controls whether the project explorer / file tree appears in the tools panel.
    show_project_explorer: ShowProjectExplorer {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.show_project_explorer",
        description: "Whether the project explorer is shown in the tools panel.",
    },
    // Controls whether global file search appears in the tools panel.
    show_global_search: ShowGlobalSearch {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.show_global_search",
        description: "Whether global file search is shown in the tools panel.",
    },
    // Controls whether hidden files (dotfiles) are shown in the project explorer.
    show_hidden_files: ShowHiddenFiles {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.show_hidden_files",
        description: "Whether hidden files (dotfiles) are shown in the project explorer.",
    },
    // Controls whether the language server reformats the file on save.
    format_on_save: FormatOnSave {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.format_on_save",
        description: "Whether the language server automatically formats the file on save. Other LSP features (hover, go-to-definition, references, diagnostics) are unaffected.",
    },
    // Controls whether the Torch text editor automatically saves file changes as the
    // user types (debounced) and when the editor loses focus. Only applies to the
    // Torch text editor, not the command line or AI input.
    auto_save: AutoSave {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "code.editor.auto_save",
        description: "Whether the Torch text editor automatically saves changes as you type and when the editor loses focus.",
    },
]);
