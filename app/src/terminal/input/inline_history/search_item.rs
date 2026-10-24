use chrono::{DateTime, Local};
use ordered_float::OrderedFloat;
use torch_core::ui::Icon;
use torch_core::ui::color::coloru_with_opacity;
use torch_core::ui::theme::Fill;
use torchui::elements::{ConstrainedBox, Container, Highlight, ParentElement, Shrinkable, Text};
use torchui::fonts::{Properties, Weight};
use torchui::prelude::{Align, CrossAxisAlignment, Flex, MainAxisAlignment, MainAxisSize};
use torchui::scene::{CornerRadius, Radius};
use torchui::text_layout::ClipConfig;
use torchui::{AppContext, Element, SingletonEntity};

use crate::agent_status::STATUS_ELEMENT_PADDING;
use crate::appearance::Appearance;
use crate::search::{ItemHighlightState, SearchItem};
use crate::terminal::history::LinkedWorkflowData;
use crate::terminal::input::inline_history::data_source::AcceptHistoryItem;
use crate::terminal::input::inline_menu::styles as inline_styles;
use crate::util::time_format::format_approx_duration_from_now_utc;

#[derive(Debug, Clone)]
pub struct InlineHistoryItem {
    command: String,
    linked_workflow_data: Option<LinkedWorkflowData>,
    prefix_match_len: usize,
    score: OrderedFloat<f64>,
    timestamp: DateTime<Local>,
}

impl InlineHistoryItem {
    pub fn command(
        command: String,
        linked_workflow_data: Option<LinkedWorkflowData>,
        timestamp: DateTime<Local>,
    ) -> Self {
        Self {
            command,
            linked_workflow_data,
            prefix_match_len: 0,
            score: OrderedFloat(f64::MIN),
            timestamp,
        }
    }

    pub fn with_prefix_match_len(mut self, len: usize) -> Self {
        self.prefix_match_len = len;
        self
    }

    pub fn with_score(mut self, score: OrderedFloat<f64>) -> Self {
        self.score = score;
        self
    }
}

impl SearchItem for InlineHistoryItem {
    type Action = AcceptHistoryItem;

    fn render_icon(
        &self,
        _highlight_state: ItemHighlightState,
        appearance: &Appearance,
    ) -> Box<dyn Element> {
        let icon_size = inline_styles::font_size(appearance);
        let icon_color = inline_styles::icon_color(appearance);
        let icon = Container::new(
            ConstrainedBox::new(Icon::Terminal.to_warpui_icon(icon_color).finish())
                .with_width(icon_size)
                .with_height(icon_size)
                .finish(),
        )
        .with_uniform_padding(STATUS_ELEMENT_PADDING)
        .with_background(coloru_with_opacity(icon_color.into(), 10))
        .with_corner_radius(CornerRadius::with_all(Radius::Pixels(
            inline_styles::ITEM_CORNER_RADIUS,
        )))
        .finish();

        Container::new(icon)
            .with_margin_right(inline_styles::ICON_MARGIN)
            .finish()
    }

    fn render_item(
        &self,
        _highlight_state: ItemHighlightState,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::handle(app).as_ref(app);
        let theme = appearance.theme();
        let font_size = inline_styles::font_size(appearance);
        let background_color = inline_styles::menu_background_color(app);

        let primary_text_color = inline_styles::primary_text_color(theme, background_color.into());
        let secondary_text_color =
            inline_styles::secondary_text_color(theme, background_color.into());

        let match_indices: Vec<usize> = (0..self.prefix_match_len).collect();

        let mut text = Text::new_inline(
            self.command.clone(),
            appearance.monospace_font_family(),
            font_size,
        )
        .with_color(primary_text_color.into())
        .with_clip(ClipConfig::ellipsis());

        if !match_indices.is_empty() {
            text = text.with_single_highlight(
                Highlight::new().with_properties(Properties::default().weight(Weight::Bold)),
                match_indices,
            );
        }

        let mut primary_row = Flex::row()
            .with_main_axis_size(MainAxisSize::Max)
            .with_main_axis_alignment(MainAxisAlignment::SpaceBetween)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_child(Shrinkable::new(1., text.finish()).finish());

        let timestamp = Text::new_inline(
            format_approx_duration_from_now_utc(self.timestamp.to_utc()),
            appearance.ui_font_family(),
            font_size,
        )
        .with_color(secondary_text_color.into())
        .finish();

        let max_timestamp_width = app
            .font_cache()
            .em_width(appearance.ui_font_family(), font_size)
            * 10.;
        primary_row.add_child(
            ConstrainedBox::new(Align::new(timestamp).right().finish())
                .with_width(max_timestamp_width)
                .finish(),
        );

        primary_row.finish()
    }

    fn item_background(
        &self,
        highlight_state: ItemHighlightState,
        appearance: &Appearance,
    ) -> Option<Fill> {
        inline_styles::item_background(highlight_state, appearance)
    }

    fn score(&self) -> OrderedFloat<f64> {
        self.score
    }

    fn accept_result(&self) -> Self::Action {
        AcceptHistoryItem {
            command: self.command.clone(),
            linked_workflow_data: self.linked_workflow_data.clone(),
        }
    }

    fn execute_result(&self) -> Self::Action {
        self.accept_result()
    }

    fn accessibility_label(&self) -> String {
        format!("Command: {}", self.command)
    }
}
