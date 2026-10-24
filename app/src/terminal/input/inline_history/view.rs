//! Inline history menu view for up-arrow command history.
use std::collections::HashSet;

use torchui::elements::ChildView;
use torchui::{AppContext, Element, Entity, ModelHandle, View, ViewContext, ViewHandle};

use crate::features::FeatureFlag;
use crate::search::data_source::{Query, QueryFilter};
use crate::search::mixer::{SearchMixer, SearchMixerEvent};
use crate::terminal::history::LinkedWorkflowData;
use crate::terminal::input::buffer_model::{InputBufferModel, InputBufferUpdateEvent};
use crate::terminal::input::inline_history::data_source::{
    AcceptHistoryItem, InlineHistoryMenuDataSource,
};
use crate::terminal::input::inline_menu::{
    InlineMenuEvent, InlineMenuHeaderConfig, InlineMenuModel, InlineMenuPositioner,
    InlineMenuTabConfig, InlineMenuView,
};
use crate::terminal::input::suggestions_mode_model::{
    InputSuggestionsModeEvent, InputSuggestionsModeModel,
};
use crate::terminal::model::session::active_session::ActiveSession;

#[derive(Debug, Clone)]
pub enum InlineHistoryMenuEvent {
    AcceptCommand {
        command: String,
        linked_workflow_data: Option<LinkedWorkflowData>,
    },
    SelectCommand {
        command: String,
        linked_workflow_data: Option<LinkedWorkflowData>,
    },
    NoResults,
    /// Emitted when the inline menu should be closed and additionally restore the
    /// original input buffer contents.
    Close,
}

/// Identifies a history item well enough to reselect the same logical item
/// after rerunning the current query.
#[derive(Clone)]
struct HistoryItemIdentity {
    command: String,
}

impl HistoryItemIdentity {
    fn from_item(item: &AcceptHistoryItem) -> Self {
        Self {
            command: item.command.clone(),
        }
    }

    fn matches(&self, item: &AcceptHistoryItem) -> bool {
        self.command == item.command
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryTab {
    All,
}

pub struct InlineHistoryMenuView {
    menu_view: ViewHandle<InlineMenuView<AcceptHistoryItem, HistoryTab>>,
    mixer: ModelHandle<SearchMixer<AcceptHistoryItem>>,
    model: ModelHandle<InlineMenuModel<AcceptHistoryItem, HistoryTab>>,
    buffer_model: ModelHandle<InputBufferModel>,
    pending_tab_switch_selection: Option<HistoryItemIdentity>,
    pending_initial_buffer_sync: bool,
}

impl InlineHistoryMenuView {
    pub fn new(
        active_session: ModelHandle<ActiveSession>,
        input_suggestions_model: &ModelHandle<InputSuggestionsModeModel>,
        positioner: &ModelHandle<InlineMenuPositioner>,
        buffer_model: ModelHandle<InputBufferModel>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let tab_configs = vec![InlineMenuTabConfig {
            id: HistoryTab::All,
            label: "All".to_string(),
            filters: HashSet::new(),
        }];
        let data_source = ctx.add_model(|_| InlineHistoryMenuDataSource::new(active_session));

        let initial_filters = tab_configs
            .first()
            .map(|config| config.filters.clone())
            .unwrap_or_default();

        let mixer = ctx.add_model(|ctx| {
            let mut mixer = SearchMixer::<AcceptHistoryItem>::new();
            mixer.add_sync_source(data_source, [QueryFilter::History]);
            mixer.run_query(
                Query {
                    text: String::new(),
                    filters: initial_filters,
                },
                ctx,
            );
            mixer
        });

        let menu_view = if FeatureFlag::InlineMenuHeaders.is_enabled() {
            let header_config = InlineMenuHeaderConfig {
                label: "History".to_string(),
                trailing_element: None,
            };
            ctx.add_typed_action_view(|ctx| {
                InlineMenuView::new_with_tabs(
                    mixer.clone(),
                    positioner.clone(),
                    input_suggestions_model,
                    tab_configs,
                    None,
                    ctx,
                )
                .with_header_config(header_config)
            })
        } else {
            ctx.add_typed_action_view(|ctx| {
                InlineMenuView::new_with_tabs(
                    mixer.clone(),
                    positioner.clone(),
                    input_suggestions_model,
                    tab_configs,
                    None,
                    ctx,
                )
            })
        };
        let model = menu_view.as_ref(ctx).model().clone();

        ctx.subscribe_to_model(input_suggestions_model, |me, model, event, ctx| {
            let InputSuggestionsModeEvent::ModeChanged { .. } = event;
            if model.as_ref(ctx).is_inline_history_menu() {
                me.open_with_current_buffer(ctx);
            }
        });

        let suggestions_mode_model_for_buffer = input_suggestions_model.clone();
        ctx.subscribe_to_model(
            &buffer_model,
            move |me, _, _: &InputBufferUpdateEvent, ctx| {
                if !suggestions_mode_model_for_buffer
                    .as_ref(ctx)
                    .is_inline_history_menu()
                {
                    return;
                }
                if !me.pending_initial_buffer_sync {
                    return;
                }
                me.pending_initial_buffer_sync = false;
                me.open_with_current_buffer(ctx);
            },
        );

        ctx.subscribe_to_view(&menu_view, |me, _, event, ctx| match event {
            InlineMenuEvent::AcceptedItem { item, .. } => {
                ctx.emit(InlineHistoryMenuEvent::AcceptCommand {
                    command: item.command.clone(),
                    linked_workflow_data: item.linked_workflow_data.clone(),
                });
            }
            InlineMenuEvent::SelectedItem { item } => {
                ctx.emit(InlineHistoryMenuEvent::SelectCommand {
                    command: item.command.clone(),
                    linked_workflow_data: item.linked_workflow_data.clone(),
                });
            }
            InlineMenuEvent::Dismissed => {
                ctx.emit(InlineHistoryMenuEvent::Close);
            }
            InlineMenuEvent::NoResults => {
                ctx.emit(InlineHistoryMenuEvent::NoResults);
            }
            InlineMenuEvent::TabChanged => {
                me.pending_tab_switch_selection = me
                    .model
                    .as_ref(ctx)
                    .selected_item()
                    .map(HistoryItemIdentity::from_item);
                me.rerun_query(ctx);
            }
        });

        let suggestions_mode_model = input_suggestions_model.clone();
        ctx.subscribe_to_model(&mixer, move |me, _, event, ctx| {
            let SearchMixerEvent::ResultsChanged = event;
            if !suggestions_mode_model.as_ref(ctx).is_inline_history_menu() {
                return;
            }
            // Only tab switches stash a pending selection to restore after the
            // query reruns. For all other result updates, keep the inline menu's
            // default selection behavior.
            let Some(selection) = me.pending_tab_switch_selection.take() else {
                return;
            };
            me.menu_view.update(ctx, |menu, ctx| {
                menu.select_last_where(|item| selection.matches(item), ctx);
            });
        });

        Self {
            menu_view,
            mixer,
            model,
            buffer_model,
            pending_tab_switch_selection: None,
            pending_initial_buffer_sync: false,
        }
    }

    /// Returns the model handle for external use (e.g., by message bars).
    pub fn model(&self) -> &ModelHandle<InlineMenuModel<AcceptHistoryItem, HistoryTab>> {
        &self.model
    }

    pub fn render_results_only(&self, app: &AppContext) -> Box<dyn Element> {
        self.menu_view.as_ref(app).render_results_only(
            /* should_render_results_in_reverse */ false, /* horizontal_padding */ 0.,
            app,
        )
    }

    pub fn result_count(&self, app: &AppContext) -> usize {
        self.menu_view.as_ref(app).result_count()
    }

    pub fn select_up(&self, ctx: &mut ViewContext<Self>) {
        self.menu_view.update(ctx, |v, ctx| v.select_up(ctx));
    }

    pub fn select_down(&self, ctx: &mut ViewContext<Self>) {
        let should_close = self.menu_view.read(ctx, |v, _| {
            let result_count = v.result_count();
            let is_last_item_selected =
                result_count > 0 && v.selected_idx().is_some_and(|idx| idx == result_count - 1);
            is_last_item_selected || result_count == 0
        });

        if should_close {
            ctx.emit(InlineHistoryMenuEvent::Close);
        } else {
            self.menu_view.update(ctx, |v, ctx| v.select_down(ctx));
        }
    }

    pub fn select_next_tab(&self, ctx: &mut ViewContext<Self>) -> bool {
        self.menu_view.update(ctx, |v, ctx| v.select_next_tab(ctx))
    }

    pub fn accept_selected_item(&self, ctx: &mut ViewContext<Self>) {
        self.menu_view
            .update(ctx, |v, ctx| v.accept_selected_item(false, ctx));
    }

    pub fn arm_initial_buffer_sync(&mut self) {
        self.pending_initial_buffer_sync = true;
    }

    fn open_with_current_buffer(&mut self, ctx: &mut ViewContext<Self>) {
        let query_text = self.buffer_model.as_ref(ctx).current_value().to_owned();
        let filters = self.model.as_ref(ctx).active_tab_filters();
        self.mixer.update(ctx, |mixer, ctx| {
            mixer.run_query(
                Query {
                    text: query_text,
                    filters,
                },
                ctx,
            );
        });
    }

    fn rerun_query(&self, ctx: &mut ViewContext<Self>) {
        let filters = self.model.as_ref(ctx).active_tab_filters();
        let query_text = self.current_query_text(ctx);
        self.mixer.update(ctx, |mixer, ctx| {
            mixer.run_query(
                Query {
                    text: query_text,
                    filters,
                },
                ctx,
            );
        });
    }

    fn current_query_text(&self, ctx: &ViewContext<Self>) -> String {
        // Read the query text from the mixer rather than the editor buffer. While the
        // inline history menu is open, moving the highlighted row can preview that
        // item in the editor buffer. When we rerun the search after a tab change, we
        // want to preserve the user's typed query, not the temporary preview text.
        self.mixer
            .as_ref(ctx)
            .current_query()
            .map(|q| q.text.clone())
            .unwrap_or_default()
    }
}

impl View for InlineHistoryMenuView {
    fn ui_name() -> &'static str {
        "InlineHistoryMenuView"
    }

    fn render(&self, _app: &torchui::AppContext) -> Box<dyn Element> {
        ChildView::new(&self.menu_view).finish()
    }
}

impl Entity for InlineHistoryMenuView {
    type Event = InlineHistoryMenuEvent;
}
