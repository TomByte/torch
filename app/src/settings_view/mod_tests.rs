use pathfinder_geometry::vector::vec2f;
use settings_page::{
    Category, CategoryHeader, FilteredPageType, MatchData, PageTitle, PageType, SettingsWidget,
    categories_with_visible_content, search_terms_match,
};
use torchui::elements::Empty;
use torchui::platform::WindowStyle;
use torchui::{
    App, AppContext, Element, Entity, Presenter, TypedActionView, View, WindowInvalidation,
};

use super::*;
use crate::appearance::Appearance;

// ── MatchData behavior ──────────────────────────────────────────────────────

#[test]
fn match_data_uncounted_true_is_truthy() {
    assert!(MatchData::Uncounted(true).is_truthy());
}

#[test]
fn match_data_uncounted_false_is_not_truthy() {
    assert!(!MatchData::Uncounted(false).is_truthy());
}

#[test]
fn match_data_countable_nonzero_is_truthy() {
    assert!(MatchData::Countable(3).is_truthy());
    assert!(MatchData::Countable(1).is_truthy());
}

#[test]
fn match_data_countable_zero_is_not_truthy() {
    assert!(!MatchData::Countable(0).is_truthy());
}

// ── Display labels ─────────────────────────────────────────────────

#[test]
fn display_names_are_correct() {
    assert_eq!(SettingsSection::Projects.to_string(), "Projects");
    assert_eq!(SettingsSection::ThirdPartyCLIAgents.to_string(), "Agents");
    assert_eq!(SettingsSection::Privacy.to_string(), "Secret redaction");
    assert_eq!(
        SettingsSection::Keybindings.to_string(),
        "Keyboard shortcuts"
    );
}

// ── slug / from_slug ───────────────────────────────────────────────

/// Every `SettingsSection` variant.
///
/// `all_sections_list_is_exhaustive` keeps this honest: adding a variant
/// breaks the exhaustive match there, which is the prompt to add it here.
const ALL_SECTIONS: &[SettingsSection] = &[
    SettingsSection::About,
    SettingsSection::Appearance,
    SettingsSection::Features,
    SettingsSection::Keybindings,
    SettingsSection::Privacy,
    SettingsSection::Scripting,
    SettingsSection::Projects,
    SettingsSection::ThirdPartyCLIAgents,
];

/// Sections whose user-facing Display label has deliberately diverged from the
/// slug it was seeded from, because the slug is a stored contract that the
/// rename must not follow.
const SECTIONS_WITH_RENAMED_DISPLAY_LABELS: &[SettingsSection] = &[
    SettingsSection::ThirdPartyCLIAgents,
    SettingsSection::Privacy,
];

#[test]
fn all_sections_list_is_exhaustive() {
    fn is_listed(section: SettingsSection) -> bool {
        let known = match section {
            SettingsSection::About
            | SettingsSection::Appearance
            | SettingsSection::Features
            | SettingsSection::Keybindings
            | SettingsSection::Privacy
            | SettingsSection::Scripting
            | SettingsSection::Projects
            | SettingsSection::ThirdPartyCLIAgents => section,
        };
        ALL_SECTIONS.contains(&known)
    }

    for section in ALL_SECTIONS {
        assert!(is_listed(*section), "{section:?} is missing from the list");
    }
}

#[test]
fn every_section_round_trips_through_its_slug() {
    for section in ALL_SECTIONS {
        assert_eq!(
            SettingsSection::from_slug(section.slug()),
            Some(*section),
            "{section:?} should round-trip through its slug"
        );
    }
}

#[test]
fn slugs_are_unique_across_sections() {
    let mut slugs: Vec<&str> = ALL_SECTIONS.iter().map(|section| section.slug()).collect();
    let total = slugs.len();
    slugs.sort_unstable();
    slugs.dedup();
    assert_eq!(slugs.len(), total, "two sections share a slug");
}

#[test]
fn slugs_were_seeded_from_the_display_labels_they_replaced() {
    // Slugs were seeded from the Display strings that used to double as the
    // persistence key, so no data migration was needed. Display is now free to
    // diverge; when it does, list the section in
    // SECTIONS_WITH_RENAMED_DISPLAY_LABELS rather than moving the slug, which
    // is a stored contract.
    for section in ALL_SECTIONS {
        if SECTIONS_WITH_RENAMED_DISPLAY_LABELS.contains(section) {
            continue;
        }
        assert_eq!(
            section.slug(),
            section.to_string(),
            "{section:?} slug diverged from the Display label it was seeded from"
        );
    }
}

#[test]
fn from_slug_accepts_legacy_spellings() {
    assert_eq!(
        SettingsSection::from_slug("ThirdPartyCLIAgents"),
        Some(SettingsSection::ThirdPartyCLIAgents)
    );
}

#[test]
fn from_slug_rejects_unknown_input() {
    assert_eq!(SettingsSection::from_slug("Not a page"), None);
    assert_eq!(SettingsSection::from_slug(""), None);
}

#[test]
fn from_slug_rejects_removed_pages() {
    for slug in [
        "Warpify",
        "Editor and Code Review",
        "EditorAndCodeReview",
        "Account",
        "Billing and usage",
        "Referrals",
        "Shared blocks",
        "Teams",
        "Torch Drive",
        "Torch Agent",
        "Profiles",
        "MCP servers",
        "Knowledge",
        "Indexing and projects",
        "Environments",
        "Oz Cloud API Keys",
    ] {
        assert_eq!(SettingsSection::from_slug(slug), None, "{slug:?}");
    }
}

// ── next_nav_index wrapping ─────────────────────────────────────────────────

#[test]
fn next_nav_index_wraps_at_ends() {
    assert_eq!(next_nav_index(0, 3, CycleDirection::Up), 2);
    assert_eq!(next_nav_index(2, 3, CycleDirection::Down), 0);
    assert_eq!(next_nav_index(1, 3, CycleDirection::Up), 0);
    assert_eq!(next_nav_index(1, 3, CycleDirection::Down), 2);
}

#[test]
fn next_nav_index_handles_single_page() {
    assert_eq!(next_nav_index(0, 1, CycleDirection::Up), 0);
    assert_eq!(next_nav_index(0, 1, CycleDirection::Down), 0);
}

// ── PageType filter lifecycle across a rebuild (APP-4922) ────────────────────
// Rebuilding a page's PageType resets its widget filter to every widget, so an
// active query has to be reapplied for only matching widgets to render. No page
// rebuilds itself on navigation any more (each subpage owns its own view), but
// these tests still pin the underlying PageType::Uncategorized filter lifecycle
// and the real search_terms_match predicate that the invariant rests on.

/// Minimal View so PageType<V> can be instantiated in a unit test without the
/// full SettingsView/ViewContext a real settings page requires.
struct TestSettingsView;

impl Entity for TestSettingsView {
    type Event = ();
}

impl View for TestSettingsView {
    fn ui_name() -> &'static str {
        "TestSettingsView"
    }

    fn render(&self, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

/// A SettingsWidget whose only test-relevant state is its search terms; render
/// is never invoked by the filter lifecycle under test.
struct StubWidget {
    terms: &'static str,
}

impl SettingsWidget for StubWidget {
    type View = TestSettingsView;

    fn search_terms(&self) -> &str {
        self.terms
    }

    fn render(&self, _: &Self::View, _: &Appearance, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

/// A fresh Uncategorized page mirroring build_page -> new_uncategorized: every
/// widget index visible by default.
fn stub_widgets_page() -> PageType<TestSettingsView> {
    let widgets: Vec<Box<dyn SettingsWidget<View = TestSettingsView>>> = vec![
        Box::new(StubWidget {
            terms: "torch agent global ai toggle",
        }),
        Box::new(StubWidget {
            terms: "active ai autosuggestions prompt",
        }),
        Box::new(StubWidget {
            terms: "ai input model api key",
        }),
        Box::new(StubWidget {
            terms: "file search fuzzy opener",
        }),
        Box::new(StubWidget {
            terms: "voice input",
        }),
    ];
    PageType::new_uncategorized(widgets, None)
}

/// Number of widgets the page would render under its current filter.
fn visible_widget_count<V: View>(page: &PageType<V>) -> usize {
    let FilteredPageType::Uncategorized { widgets, .. } = page.get_filtered() else {
        panic!("expected Uncategorized page");
    };
    widgets.len()
}

#[test]
fn search_terms_match_direct_unit_checks() {
    // Empty query matches everything (mirrors PageType::update_filter's guard).
    assert!(search_terms_match("torch agent global ai toggle", ""));
    // All-words, case-insensitive, non-contiguous.
    assert!(search_terms_match(
        "active ai autosuggestions prompt",
        "autosuggestions"
    ));
    assert!(search_terms_match(
        "active ai autosuggestions prompt",
        "ACTIVE AI"
    ));
    assert!(search_terms_match(
        "file search fuzzy opener",
        "file search"
    ));
    // Every word must appear.
    assert!(!search_terms_match(
        "torch agent global ai toggle",
        "file search"
    ));
    assert!(!search_terms_match(
        "active ai autosuggestions prompt",
        "autosuggestions key"
    ));
}

#[test]
fn rebuild_resets_filter_to_all_widgets() {
    // Searching "file search" matches exactly one widget. A freshly built page
    // (mirroring build_page -> new_uncategorized) resets the filter to every
    // widget, so without reapplying update_filter the page would show all
    // widgets.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            let md = page.update_filter("file search", ctx);
            assert!(md.is_truthy());
            assert_eq!(visible_widget_count(&page), 1);

            let rebuilt = stub_widgets_page();
            assert_eq!(
                visible_widget_count(&rebuilt),
                5,
                "rebuild resets the filter to all widgets when update_filter isn't reapplied"
            );
        });
    });
}

#[test]
fn rebuild_with_reapply_keeps_only_matching_widgets() {
    // The fix: after a rebuild, reapply update_filter with the active query so
    // only matching widgets render on the restored subpage.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            page.update_filter("file search", ctx);
            assert_eq!(visible_widget_count(&page), 1);

            let mut rebuilt = stub_widgets_page();
            rebuilt.update_filter("file search", ctx);
            assert_eq!(
                visible_widget_count(&rebuilt),
                1,
                "reapplying the filter after a rebuild keeps only matching widgets visible"
            );
        });
    });
}

#[test]
fn reapply_handles_multi_word_and_case() {
    // A multi-word, case-insensitive query survives the rebuild + reapply cycle.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            page.update_filter("AI INPUT", ctx);
            assert_eq!(visible_widget_count(&page), 1);

            let mut rebuilt = stub_widgets_page();
            rebuilt.update_filter("AI INPUT", ctx);
            assert_eq!(visible_widget_count(&rebuilt), 1);
        });
    });
}

#[test]
fn empty_query_after_reapply_shows_all_widgets() {
    // When the search is cleared, the subpage shows all widgets again.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = stub_widgets_page();
            page.update_filter("agent", ctx);
            assert_eq!(visible_widget_count(&page), 1);

            let mut rebuilt = stub_widgets_page();
            rebuilt.update_filter("", ctx);
            assert_eq!(
                visible_widget_count(&rebuilt),
                5,
                "an empty query restores every widget on the subpage"
            );
        });
    });
}

struct NeverRendersWidget {
    terms: &'static str,
}

impl SettingsWidget for NeverRendersWidget {
    type View = TestSettingsView;

    fn search_terms(&self) -> &str {
        self.terms
    }

    fn should_render(&self, _: &AppContext) -> bool {
        false
    }

    fn render(&self, _: &Self::View, _: &Appearance, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
}

#[test]
fn category_whose_sole_widget_cannot_render_has_no_visible_content_before_any_filter_pass() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let children: Vec<Box<dyn SettingsWidget<View = TestSettingsView>>> =
                vec![Box::new(NeverRendersWidget {
                    terms: "cloud handoff",
                })];
            let page =
                PageType::new_categorized(vec![Category::new("Cloud Handoff", children)], None);

            let FilteredPageType::Categorized { categories, .. } = page.get_filtered() else {
                panic!("expected Categorized page");
            };
            assert_eq!(
                categories.len(),
                1,
                "the untouched filter includes every widget index, so the category is still present here"
            );
            assert!(
                categories_with_visible_content(categories, ctx).is_empty(),
                "the category's sole widget can't render right now, so it has nothing visible to show"
            );
        });
    });
}

#[test]
fn category_whose_sole_widget_cannot_render_has_no_visible_content_after_an_empty_query() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let children: Vec<Box<dyn SettingsWidget<View = TestSettingsView>>> =
                vec![Box::new(NeverRendersWidget {
                    terms: "cloud handoff",
                })];
            let mut page =
                PageType::new_categorized(vec![Category::new("Cloud Handoff", children)], None);
            page.update_filter("", ctx);

            let FilteredPageType::Categorized { categories, .. } = page.get_filtered() else {
                panic!("expected Categorized page");
            };
            assert!(
                categories.is_empty(),
                "an empty-query filter pass already drops a category with no should_render widgets"
            );
        });
    });
}

/// A no-observable-output trailing-element closure, for testing attachment and visibility only.
fn stub_trailing_element(_: &TestSettingsView, _: &Appearance, _: &AppContext) -> Box<dyn Element> {
    Empty::new().finish()
}

/// An Uncategorized page with one widget plus a title trailing element.
fn uncategorized_page_with_title_trailing_element() -> PageType<TestSettingsView> {
    let widgets: Vec<Box<dyn SettingsWidget<View = TestSettingsView>>> =
        vec![Box::new(StubWidget {
            terms: "unrelated child setting",
        })];
    PageType::new_uncategorized(
        widgets,
        Some(PageTitle::new("Page").with_trailing_element(stub_trailing_element)),
    )
}

#[test]
fn title_trailing_element_is_present_regardless_of_widget_filter() {
    // The title trailing element takes no part in search: it must be present whether or not any
    // body widget matches, and must never affect MatchData.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = uncategorized_page_with_title_trailing_element();
            let match_data = page.update_filter("totally unrelated query", ctx);
            assert!(!match_data.is_truthy());

            let FilteredPageType::Uncategorized { widgets, title, .. } = page.get_filtered() else {
                panic!("expected Uncategorized page");
            };
            assert!(widgets.is_empty());
            assert!(title.is_some_and(|t| t.trailing_element.is_some()));
        });
    });
}

/// A Categorized page with one category holding two child widgets and a trailing element.
fn categorized_page_with_trailing() -> PageType<TestSettingsView> {
    let children: Vec<Box<dyn SettingsWidget<View = TestSettingsView>>> = vec![
        Box::new(StubWidget {
            terms: "child one settings",
        }),
        Box::new(StubWidget {
            terms: "child two settings",
        }),
    ];
    let category = Category::with_header(
        CategoryHeader::new("Master").with_trailing_element(stub_trailing_element),
        children,
    );
    PageType::new_categorized(vec![category], None)
}

/// The number of widgets and whether the trailing element is present for the sole category of a
/// `categorized_page_with_trailing`-shaped page.
fn categorized_widget_and_trailing_state<V: View>(page: &PageType<V>) -> Vec<(usize, bool)> {
    let FilteredPageType::Categorized { categories, .. } = page.get_filtered() else {
        panic!("expected Categorized page");
    };
    categories
        .into_iter()
        .map(|c| (c.widgets.len(), c.trailing_element.is_some()))
        .collect()
}

#[test]
fn category_trailing_element_renders_alongside_a_matching_child() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = categorized_page_with_trailing();
            let match_data = page.update_filter("child one", ctx);
            assert!(match_data.is_truthy());
            assert_eq!(
                categorized_widget_and_trailing_state(&page),
                vec![(1, true)]
            );
        });
    });
}

#[test]
fn category_and_its_trailing_element_are_dropped_when_no_child_matches() {
    // The trailing element takes no part in search, so visibility is decided purely by the
    // children: a query can't resurface the category through the accessory.
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = categorized_page_with_trailing();
            let match_data = page.update_filter("totally unrelated query", ctx);
            assert!(!match_data.is_truthy());
            assert_eq!(categorized_widget_and_trailing_state(&page), vec![]);
        });
    });
}

#[test]
fn category_with_trailing_element_shows_everything_on_empty_query() {
    App::test((), |mut app| async move {
        app.update(|ctx| {
            let mut page = categorized_page_with_trailing();
            let match_data = page.update_filter("", ctx);
            assert!(match_data.is_truthy());
            assert_eq!(
                categorized_widget_and_trailing_state(&page),
                vec![(2, true)]
            );
        });
    });
}

/// Renders a `categorized_page_with_trailing` page, whose category has no subtitle (a
/// `render_sub_header` header, not `render_sub_header_with_description`).
struct CategoryHeaderTrailingElementTestView;

impl Entity for CategoryHeaderTrailingElementTestView {
    type Event = ();
}

impl View for CategoryHeaderTrailingElementTestView {
    fn ui_name() -> &'static str {
        "CategoryHeaderTrailingElementTestView"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        categorized_page_with_trailing().render(&TestSettingsView, app)
    }
}

impl TypedActionView for CategoryHeaderTrailingElementTestView {
    type Action = ();
}

/// Regression test: a category header with a trailing element and no subtitle used to panic flex
/// layout (see `render_header_with_trailing_element`'s `Shrinkable` fix).
#[test]
fn category_header_with_trailing_element_and_no_subtitle_does_not_panic_flex_layout() {
    App::test((), |mut app| async move {
        let app = &mut app;
        app.add_singleton_model(|_| Appearance::mock());

        let (window_id, _view) = app.add_window(WindowStyle::NotStealFocus, |_| {
            CategoryHeaderTrailingElementTestView
        });
        let root_view_id = app
            .root_view_id(window_id)
            .expect("window should have a root view");

        let mut presenter = Presenter::new(window_id);
        let invalidation = WindowInvalidation {
            updated: [root_view_id].into_iter().collect(),
            ..Default::default()
        };

        app.update(move |ctx| {
            presenter.invalidate(invalidation, ctx);
            // Panicked here before the fix.
            presenter.build_scene(vec2f(800., 600.), 1., None, ctx);
        });
    });
}
