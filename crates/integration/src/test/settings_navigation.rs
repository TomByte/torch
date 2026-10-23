//! Integration tests for settings sidebar navigation and search.
//!
//! These pin down the user-visible behavior of the settings nav rail —
//! clicking rows, arrow-key cycling, and search filtering — so that
//! refactors of the settings page model cannot silently regress them.

use torch::integration_testing::settings::{
    assert_settings_nav_page_visible, assert_settings_section, assert_settings_widget_rendered,
    clear_settings_search, click_settings_nav_page, open_settings_page, press_settings_nav_down,
    press_settings_nav_up, type_settings_search,
};
use torch::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;
use torch::settings_view::{SettingsSection, cli_agent_settings_widget_id};

use super::{Builder, new_builder};

// ---------------------------------------------------------------------------
// Mouse navigation
// ---------------------------------------------------------------------------

/// Clicking a sidebar row selects its page.
pub fn test_settings_mouse_navigation() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(click_settings_nav_page(SettingsSection::Keybindings))
        .with_step(assert_settings_section(SettingsSection::Keybindings))
        .with_step(click_settings_nav_page(SettingsSection::About))
        .with_step(assert_settings_section(SettingsSection::About))
}

// ---------------------------------------------------------------------------
// Keyboard navigation
// ---------------------------------------------------------------------------

/// Arrowing Down moves to the next page in the sidebar.
pub fn test_settings_keyboard_navigation_down() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(press_settings_nav_down())
        .with_step(assert_settings_section(SettingsSection::Features))
}

/// Arrowing Up moves to the previous page in the sidebar.
pub fn test_settings_keyboard_navigation_up() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(press_settings_nav_up())
        .with_step(assert_settings_section(
            SettingsSection::ThirdPartyCLIAgents,
        ))
}

/// Arrowing Up from the first page wraps to the last one.
pub fn test_settings_keyboard_navigation_wraps() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Projects))
        .with_step(press_settings_nav_up())
        .with_step(assert_settings_section(SettingsSection::About))
}

// ---------------------------------------------------------------------------
// Search filtering
// ---------------------------------------------------------------------------

/// A query that only matches one page hides the other rows and moves the
/// selection onto the surviving page.
pub fn test_settings_search_filters_pages() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(type_settings_search("keyboard shortcut"))
        .with_step(assert_settings_nav_page_visible(
            SettingsSection::Keybindings,
            true,
        ))
        .with_step(assert_settings_nav_page_visible(
            SettingsSection::About,
            false,
        ))
        // Appearance no longer matches, so the selection follows the filter.
        .with_step(assert_settings_section(SettingsSection::Keybindings))
}

/// A search that matches only one page must still render that page's
/// content, not an empty pane.
///
/// Sidebar visibility and content rendering are decided separately, so a page
/// can keep its row while the content pane renders nothing. No sidebar-only
/// assertion would catch that, which is why this asserts on a widget.
pub fn test_settings_search_page_still_renders_content() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        // The CLI agent widget lives on the Agents page, and nothing has
        // rendered it yet.
        .with_step(assert_settings_widget_rendered(
            cli_agent_settings_widget_id(),
            false,
        ))
        .with_step(type_settings_search("codex"))
        .with_step(assert_settings_section(
            SettingsSection::ThirdPartyCLIAgents,
        ))
        .with_step(assert_settings_widget_rendered(
            cli_agent_settings_widget_id(),
            true,
        ))
}

/// Clearing the search restores every sidebar row.
pub fn test_settings_search_clear_restores_rows() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(open_settings_page(SettingsSection::Appearance))
        .with_step(type_settings_search("codex"))
        .with_step(assert_settings_nav_page_visible(
            SettingsSection::About,
            false,
        ))
        .with_step(clear_settings_search())
        .with_step(assert_settings_nav_page_visible(
            SettingsSection::About,
            true,
        ))
}
