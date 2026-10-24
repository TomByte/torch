use torchui::integration::{AssertionCallback, AssertionOutcome};
use torchui::{App, ViewHandle, WindowId, async_assert};

use crate::integration_testing::view_getters::{command_palette_view, workspace_view};
use crate::palette::PaletteMode;
use crate::pane_group::PaneGroup;
use crate::search::QueryFilter;
use crate::search::command_palette::ItemSummary;

/// Used to determine which tab should be the most recent in Navigation Palette integration tests.
pub enum RecentSession {
    First,
    Second,
}

/// Asserts that the navigation filter is currently enabled within the command palette.
pub fn assert_navigation_mode_enabled_in_command_palette() -> AssertionCallback {
    Box::new(move |app, window_id| {
        let workspace = workspace_view(app, window_id);

        workspace.read(app, |workspace, ctx| {
            async_assert!(
                workspace.is_palette_mode_enabled(PaletteMode::Navigation, ctx),
                "Expected navigation palette to be enabled"
            )
        })
    })
}

/// Asserts that the command palette lists the tab that is not the most recent one (given by
/// [`RecentSession`]) ahead of the most recent tab, which holds the current session.
pub fn check_recency(
    first_pane_group: ViewHandle<PaneGroup>,
    second_pane_group: ViewHandle<PaneGroup>,
    recency_test: RecentSession,
    app: &App,
    window_id: WindowId,
) -> AssertionOutcome {
    let command_palette = command_palette_view(app, window_id);
    command_palette.read(app, |palette, app| {
        assert_eq!(
            palette.active_query_filter(app),
            Some(QueryFilter::Sessions),
            "Sessions query filter is not applied"
        );

        let mut search_results = palette.search_results(app);
        let first_item = search_results
            .next()
            .expect("first item doesn't exist in search results")
            .accept_result()
            .to_summary();
        let second_item = search_results
            .next()
            .expect("second item doesn't exist in search results")
            .accept_result()
            .to_summary();

        let (
            ItemSummary::Tab {
                pane_group_id: first_tab,
            },
            ItemSummary::Tab {
                pane_group_id: second_tab,
            },
        ) = (first_item, second_item)
        else {
            return AssertionOutcome::failure(
                "The first two items in the command palette are not tabs".to_string(),
            );
        };

        // The palette offers the previously used tab first and keeps the current one last.
        let (expected_first, expected_second) = match recency_test {
            RecentSession::First => (second_pane_group.id(), first_pane_group.id()),
            RecentSession::Second => (first_pane_group.id(), second_pane_group.id()),
        };
        async_assert!(
            first_tab == expected_first && second_tab == expected_second,
            "Tabs are not ordered by recency: got {first_tab:?}, {second_tab:?}"
        )
    })
}
