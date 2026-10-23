use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use parking_lot::FairMutex;
use torch::tui_export::{
    AIAgentExchangeId, AIAgentInput, AIBlockModel, AIBlockOutputStatus, AIConversationId,
    AIRequestType, Appearance, CancellationReason, LLMId, OutputStatusUpdateCallback,
    RichContentItem, RichContentType, ServerOutputId, TerminalModel, UserQueryMode,
};
use torchui::platform::WindowStyle;
use torchui::{AddWindowOptions, EntityIdMap, ViewHandle};
use torchui_core::elements::tui::{
    TuiConstraint, TuiGridPoint, TuiLayoutContext, TuiSelectionSpan, TuiSize, TuiViewportWindow,
    TuiViewportedElement,
};
use torchui_core::{App, AppContext, ViewContext};

use super::{AgentBlockRegistry, TuiBlockListViewportItemId, TuiBlockListViewportSource};
use crate::agent_block::TuiAIBlock;
use crate::terminal_block::should_render_terminal_block;
use crate::test_fixtures::{TestHostView, add_test_action_model_and_events};

#[test]
fn tui_block_list_viewport_source_uses_canonical_block_list_order() {
    let mut model = TerminalModel::mock(None, None);
    model.simulate_block("echo 1", "1\r\n");
    model.simulate_block("echo 2", "2\r\n");
    let expected = model
        .block_list()
        .blocks()
        .iter()
        .filter(|block| should_render_terminal_block(block, model.block_list()))
        .map(|block| TuiBlockListViewportItemId::Terminal(block.id().clone()))
        .collect::<Vec<_>>();
    let source = TuiBlockListViewportSource::new(
        Arc::new(FairMutex::new(model)),
        AgentBlockRegistry::new(RefCell::new(HashMap::new())),
    );

    let actual = source.item_ids_for_test();

    assert_eq!(actual, expected);
}

#[test]
fn tui_block_list_viewport_source_slices_terminal_blocks_to_visible_rows() {
    App::test((), |app| async move {
        app.add_singleton_model(|_| Appearance::mock());
        app.read(|app| {
            let mut model = TerminalModel::mock(None, None);
            model.simulate_block("printf", "one\r\ntwo\r\nthree\r\n");
            let source = TuiBlockListViewportSource::new(
                Arc::new(FairMutex::new(model)),
                AgentBlockRegistry::new(RefCell::new(HashMap::new())),
            );

            let mut rendered_views = EntityIdMap::default();
            let mut layout_ctx = TuiLayoutContext {
                rendered_views: &mut rendered_views,
            };
            let content = source.visible_items(
                TuiViewportWindow {
                    scroll_top: 1,
                    viewport_height: 1,
                },
                80,
                &mut layout_ctx,
                app,
            );

            assert_eq!(content.items.len(), 1);
            let mut item = content.items.into_iter().next().unwrap();
            assert_eq!(item.origin_y, 1);

            let size = item.element.layout(
                TuiConstraint::loose(TuiSize::new(80, u16::MAX)),
                &mut layout_ctx,
                app,
            );
            assert_eq!(size.height, 1);
        });
    });
}

/// A selection covering the whole wrapped input section copies the original
/// query verbatim (logical text), not the per-row grid scrape. Guards the TUI
/// copy bug where a multi-row selection pasted with inserted newlines.
#[test]
fn selection_logical_text_returns_query_for_full_input_selection() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());
        // `seeded_agent_block_source` registers a block whose input query is
        // "hello world from rust".
        let query = "hello world from rust";
        let (source, _model, agent_block) = seeded_agent_block_source(&mut app, 0, 99.0);
        let width = 12u16;
        let block_height = app.read(|app| {
            let mut rendered_views = EntityIdMap::default();
            let mut ctx = TuiLayoutContext {
                rendered_views: &mut rendered_views,
            };
            agent_block.as_ref(app).desired_height(width, &mut ctx, app)
        });
        // The block is input-only, so its height is the top padding row plus the
        // wrapped input; a wrapped input spans more than one row here.
        assert!(
            block_height > 2,
            "input should wrap at width {width}: {block_height}"
        );
        let selection = TuiSelectionSpan {
            start: TuiGridPoint { row: 1, col: 0 },
            end: TuiGridPoint {
                row: block_height - 1,
                col: width,
            },
        };

        let text = app.read(|app| source.selection_logical_text(selection, width, app));

        assert_eq!(text.as_deref(), Some(query));
    });
}

/// A selection that does not cover the whole input section (here starting
/// mid-line) returns `None`, so copy falls back to the per-row grid path.
#[test]
fn selection_logical_text_falls_back_for_partial_input_selection() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());
        let (source, _model, _agent_block) = seeded_agent_block_source(&mut app, 0, 99.0);
        let width = 12u16;
        let selection = TuiSelectionSpan {
            start: TuiGridPoint { row: 1, col: 1 },
            end: TuiGridPoint { row: 2, col: width },
        };

        let text = app.read(|app| source.selection_logical_text(selection, width, app));

        assert_eq!(text, None);
    });
}

/// A selection that ends part-way through the section's final wrapped row (the
/// end column stops short of the last glyph) falls back to per-row grid text,
/// so the whole logical section is not returned and unselected trailing text is
/// never copied.
#[test]
fn selection_logical_text_falls_back_for_partial_end_selection() {
    App::test((), |mut app| async move {
        app.add_singleton_model(|_| Appearance::mock());
        let (source, _model, agent_block) = seeded_agent_block_source(&mut app, 0, 99.0);
        let width = 12u16;
        let block_height = app.read(|app| {
            let mut rendered_views = EntityIdMap::default();
            let mut ctx = TuiLayoutContext {
                rendered_views: &mut rendered_views,
            };
            agent_block.as_ref(app).desired_height(width, &mut ctx, app)
        });
        assert!(
            block_height > 2,
            "input should wrap at width {width}: {block_height}"
        );
        // Start at the section's first column, but end at column 1 of the last
        // rendered row — short of the last glyph.
        let selection = TuiSelectionSpan {
            start: TuiGridPoint { row: 1, col: 0 },
            end: TuiGridPoint {
                row: block_height - 1,
                col: 1,
            },
        };

        let text = app.read(|app| source.selection_logical_text(selection, width, app));

        assert_eq!(text, None);
    });
}

/// Builds a source over one registered agent block seeded with a stale,
/// non-dirty cached height. When `preceding_rows > 0`, a terminal block of that
/// many output rows precedes it, controlling how far below the top it sits.
fn seeded_agent_block_source(
    app: &mut App,
    preceding_rows: usize,
    stale_height: f64,
) -> (
    TuiBlockListViewportSource,
    Arc<FairMutex<TerminalModel>>,
    ViewHandle<TuiAIBlock>,
) {
    let mut model = TerminalModel::mock(None, None);
    if preceding_rows > 0 {
        model.simulate_block("printf", &"x\r\n".repeat(preceding_rows));
    }
    let terminal_model = Arc::new(FairMutex::new(model));
    let agent_block = add_agent_block(app, "hello world from rust");
    let view_id = agent_block.id();
    {
        let mut model = terminal_model.lock();
        model.block_list_mut().append_rich_content(
            RichContentItem::new(Some(RichContentType::AIBlock), view_id, None, false),
            false,
        );
        // Clear the dirty flag and seed a stale height so only re-measurement
        // (not the dirty path) can correct it.
        model.block_list_mut().take_dirty_rich_content_items();
        model
            .block_list_mut()
            .update_rich_content_heights(&HashMap::from([(view_id, stale_height)]));
    }
    let agent_blocks = AgentBlockRegistry::new(RefCell::new(HashMap::from([(
        view_id,
        agent_block.clone(),
    )])));
    let source = TuiBlockListViewportSource::new(terminal_model.clone(), agent_blocks);
    (source, terminal_model, agent_block)
}

/// Adds a finished `TuiAIBlock` backed by a single-query model in a fresh TUI window.
fn add_agent_block(app: &mut App, query: &str) -> ViewHandle<TuiAIBlock> {
    add_agent_block_with(
        app,
        vec![query_input(query)],
        non_streaming_status(),
        Arc::new(FairMutex::new(TerminalModel::mock(None, None))),
    )
}

/// Adds a `TuiAIBlock` over `inputs`/`status`, backed by `terminal_model`, in a
/// fresh TUI window. Passing the same `terminal_model` the viewport source uses
/// lets the block's interaction toggles dirty the source's canonical rich
/// content.
fn add_agent_block_with(
    app: &mut App,
    inputs: Vec<AIAgentInput>,
    status: AIBlockOutputStatus,
    terminal_model: Arc<FairMutex<TerminalModel>>,
) -> ViewHandle<TuiAIBlock> {
    let (action_model, model_events) = add_test_action_model_and_events(app);
    app.update(|ctx| {
        let (window_id, _) = ctx.add_tui_window(
            AddWindowOptions {
                window_style: WindowStyle::NotStealFocus,
                ..Default::default()
            },
            |_| TestHostView,
        );
        ctx.add_typed_action_tui_view(window_id, move |ctx| {
            TuiAIBlock::new(
                (AIConversationId::new(), AIAgentExchangeId::new()),
                Rc::new(QueryAgentBlockModel { inputs, status }),
                action_model,
                &model_events,
                terminal_model,
                false,
                ctx,
            )
        })
    })
}

/// A finished (cancelled) status: the block is not streaming, so the viewport's
/// width-gating alone decides whether to re-measure it.
fn non_streaming_status() -> AIBlockOutputStatus {
    AIBlockOutputStatus::Cancelled {
        partial_output: None,
        reason: CancellationReason::ManuallyCancelled,
    }
}

struct QueryAgentBlockModel {
    inputs: Vec<AIAgentInput>,
    status: AIBlockOutputStatus,
}

impl AIBlockModel for QueryAgentBlockModel {
    type View = TuiAIBlock;

    fn status(&self, _app: &AppContext) -> AIBlockOutputStatus {
        self.status.clone()
    }

    fn server_output_id(&self, _app: &AppContext) -> Option<ServerOutputId> {
        None
    }

    fn model_id(&self, _app: &AppContext) -> Option<LLMId> {
        None
    }

    fn base_model<'a>(&'a self, _app: &'a AppContext) -> Option<&'a LLMId> {
        None
    }

    fn inputs_to_render<'a>(&'a self, _app: &'a AppContext) -> &'a [AIAgentInput] {
        &self.inputs
    }

    fn conversation_id(&self, _app: &AppContext) -> Option<AIConversationId> {
        None
    }

    fn on_updated_output(
        &self,
        _callback: OutputStatusUpdateCallback<Self::View>,
        _ctx: &mut ViewContext<Self::View>,
    ) {
    }

    fn request_type(&self, _app: &AppContext) -> AIRequestType {
        AIRequestType::Active
    }
}

/// Builds one user-query input for wrapping-height tests.
fn query_input(query: &str) -> AIAgentInput {
    AIAgentInput::UserQuery {
        query: query.to_owned(),
        context: Default::default(),
        static_query_type: None,
        referenced_attachments: Default::default(),
        user_query_mode: UserQueryMode::default(),
        running_command: None,
        intended_agent: None,
        base: None,
    }
}
