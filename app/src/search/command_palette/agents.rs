//! Torch's Agents palette: every pane running a coding agent, most in need of the user first.
use std::sync::Arc;

use fuzzy_match::match_indices_case_insensitive;
use instant::Instant;
use ordered_float::OrderedFloat;
use parking_lot::Mutex;
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
    /// Shared with the listed items, which read their pane whenever they render.
    panes: Arc<Mutex<Vec<AgentPane>>>,
}

impl DataSource {
    pub fn set_panes(&mut self, mut panes: Vec<AgentPane>) {
        // Stable, so panes in the same state keep sidebar order.
        panes.sort_by_key(AgentPane::state);
        *self.panes.lock() = panes;
    }

    /// Brings the listed panes up to date without reordering them, so a row never moves out from
    /// under the selection. Agents that start meanwhile are listed the next time the palette opens.
    pub fn refresh_panes(&mut self, current: &[AgentPane]) {
        for pane in self.panes.lock().iter_mut() {
            if let Some(update) = current.iter().find(|update| update.locator == pane.locator) {
                *pane = update.clone();
            }
        }
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
        let panes = self.panes.lock();
        Ok(panes
            .iter()
            .enumerate()
            .filter_map(|(rank, pane)| {
                let score = if text.is_empty() {
                    // Results are shown highest score first.
                    (panes.len() - rank) as f64
                } else {
                    let searchable_text = format!("{} {}", title(pane), detail(pane, now));
                    match_indices_case_insensitive(&searchable_text, text)?.score as f64
                };
                Some(
                    Item {
                        panes: self.panes.clone(),
                        pane: pane.clone(),
                        score,
                    }
                    .into(),
                )
            })
            .collect())
    }
}

impl Entity for DataSource {
    type Event = ();
}

struct Item {
    /// The palette's panes, so the row shows its agent as it is now rather than when the query ran.
    panes: Arc<Mutex<Vec<AgentPane>>>,
    /// The pane as the query found it.
    pane: AgentPane,
    score: f64,
}

impl Item {
    fn current_pane(&self) -> AgentPane {
        self.panes
            .lock()
            .iter()
            .find(|pane| pane.locator == self.pane.locator)
            .unwrap_or(&self.pane)
            .clone()
    }
}

fn title(pane: &AgentPane) -> String {
    pane.task
        .clone()
        .unwrap_or_else(|| format!("{} session", pane.agent))
}

fn detail(pane: &AgentPane, now: Instant) -> String {
    let mut detail = format!(
        "{} · {} · {}",
        pane.agent,
        pane.project,
        pane.state().label()
    );
    if let Some(time) = pane.time_in_state(now) {
        detail.push_str(&format!(" {time}"));
    }
    detail
}

impl SearchItem for Item {
    type Action = CommandPaletteItemAction;

    fn is_multiline(&self) -> bool {
        true
    }

    fn render_icon(&self, _: ItemHighlightState, appearance: &Appearance) -> Box<dyn Element> {
        Container::new(
            ConstrainedBox::new(self.current_pane().render_status_icon(appearance))
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
        let pane = self.current_pane();
        Flex::column()
            .with_child(
                Text::new_inline(title(&pane), appearance.ui_font_family(), 14.)
                    .with_color(theme.foreground().into_solid())
                    .with_clip(ClipConfig::ellipsis())
                    .finish(),
            )
            .with_child(
                Container::new(
                    Text::new_inline(
                        detail(&pane, Instant::now()),
                        appearance.ui_font_family(),
                        12.,
                    )
                    .with_clip(ClipConfig::ellipsis())
                    .with_color(pane.state_color(appearance))
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
        let pane = self.current_pane();
        format!("{}. {}", title(&pane), detail(&pane, Instant::now()))
    }
}

#[cfg(test)]
#[path = "agents_tests.rs"]
mod tests;
