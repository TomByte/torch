//! Torch's Agents palette: every pane running a coding agent, most in need of the user first.
use std::time::Duration;

use fuzzy_match::match_indices_case_insensitive;
use instant::Instant;
use ordered_float::OrderedFloat;
use warp_core::ui::color::blend::Blend;
use warp_core::ui::theme::Fill;
use warpui::elements::{ConstrainedBox, Container, Flex, ParentElement, Text};
use warpui::text_layout::ClipConfig;
use warpui::{AppContext, Element, Entity, SingletonEntity};

use crate::appearance::Appearance;
use crate::search::command_palette::mixer::CommandPaletteItemAction;
use crate::search::data_source::{Query, QueryResult};
use crate::search::item::SearchItem;
use crate::search::mixer::{DataSourceRunErrorWrapper, SyncDataSource};
use crate::search::result_renderer::ItemHighlightState;
use crate::workspace::view::AgentPane;

#[derive(Default)]
pub struct DataSource {
    panes: Vec<AgentPane>,
}

impl DataSource {
    pub fn set_panes(&mut self, mut panes: Vec<AgentPane>) {
        // Stable, so panes in the same state keep sidebar order.
        panes.sort_by_key(AgentPane::state);
        self.panes = panes;
    }
}

impl SyncDataSource for DataSource {
    type Action = CommandPaletteItemAction;

    fn run_query(
        &self,
        query: &Query,
        _: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        let text = query.text.trim();
        let now = Instant::now();
        Ok(self
            .panes
            .iter()
            .enumerate()
            .filter_map(|(rank, pane)| {
                let item = Item::new(pane, now);
                let score = if text.is_empty() {
                    // Results are shown highest score first.
                    (self.panes.len() - rank) as f64
                } else {
                    match_indices_case_insensitive(&item.searchable_text(), text)?.score as f64
                };
                Some(Item { score, ..item }.into())
            })
            .collect())
    }
}

impl Entity for DataSource {
    type Event = ();
}

struct Item {
    pane: AgentPane,
    title: String,
    detail: String,
    score: f64,
}

impl Item {
    fn new(pane: &AgentPane, now: Instant) -> Self {
        let state = pane.state();
        let mut detail = format!("{} · {} · {}", pane.agent, pane.project, state.label());
        if let Some(since) = pane.status_since {
            detail.push_str(&format!(" {}", elapsed_label(now.duration_since(since))));
        }
        Self {
            pane: pane.clone(),
            title: pane
                .task
                .clone()
                .unwrap_or_else(|| format!("{} session", pane.agent)),
            detail,
            score: 0.,
        }
    }

    fn searchable_text(&self) -> String {
        format!("{} {}", self.title, self.detail)
    }
}

fn elapsed_label(elapsed: Duration) -> String {
    let minutes = elapsed.as_secs() / 60;
    match minutes {
        0 => "just now".to_owned(),
        1..60 => format!("{minutes}m"),
        60..1440 => format!("{}h", minutes / 60),
        _ => format!("{}d", minutes / 1440),
    }
}

impl SearchItem for Item {
    type Action = CommandPaletteItemAction;

    fn is_multiline(&self) -> bool {
        true
    }

    fn render_icon(&self, _: ItemHighlightState, appearance: &Appearance) -> Box<dyn Element> {
        Container::new(
            ConstrainedBox::new(self.pane.render_status_icon(appearance))
                .with_width(16.)
                .with_height(16.)
                .finish(),
        )
        .with_margin_right(12.)
        .finish()
    }

    fn item_background(&self, state: ItemHighlightState, appearance: &Appearance) -> Option<Fill> {
        let theme = appearance.theme();
        match state {
            ItemHighlightState::Selected { .. } => {
                Some(theme.background().blend(&theme.accent().with_opacity(16)))
            }
            ItemHighlightState::Hovered => Some(theme.surface_1()),
            ItemHighlightState::Default => None,
        }
    }

    fn render_item(&self, _: ItemHighlightState, app: &AppContext) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        Flex::column()
            .with_child(
                Text::new_inline(self.title.clone(), appearance.ui_font_family(), 14.)
                    .with_color(theme.foreground().into_solid())
                    .with_clip(ClipConfig::ellipsis())
                    .finish(),
            )
            .with_child(
                Container::new(
                    Text::new_inline(self.detail.clone(), appearance.ui_font_family(), 12.)
                        .with_clip(ClipConfig::ellipsis())
                        .with_color(self.pane.state_color(appearance))
                        .finish(),
                )
                .with_padding_top(3.)
                .finish(),
            )
            .finish()
    }

    fn score(&self) -> OrderedFloat<f64> {
        OrderedFloat(self.score)
    }

    fn accept_result(&self) -> Self::Action {
        CommandPaletteItemAction::NavigateToSession {
            pane_view_locator: self.pane.locator,
            window_id: self.pane.window_id,
        }
    }

    fn execute_result(&self) -> Self::Action {
        self.accept_result()
    }

    fn accessibility_label(&self) -> String {
        format!("{}. {}", self.title, self.detail)
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
