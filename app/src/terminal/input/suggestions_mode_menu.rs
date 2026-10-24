//! Rendering logic for the input suggestions menu.
//!
//! This module contains all rendering functions for the various input suggestion modes:
//! - HistoryUp
//! - CompletionSuggestions

use torchui::elements::{
    Border, ConstrainedBox, Container, CornerRadius, DragBarSide, DropShadow, Element, Empty, Flex,
    ParentElement, Radius, Resizable, Shrinkable, SizeConstraintCondition, SizeConstraintSwitch,
};
use torchui::presenter::ChildView;

use super::{
    HISTORY_DETAILS_VIEW_WIDTH_REQUIREMENT, Input, InputAction, MenuPositioning,
    TERMINAL_VIEW_PADDING_LEFT,
};
use crate::appearance::Appearance;
use crate::input_suggestions::{
    DETAILS_PANEL_MARGIN, DETAILS_PANEL_PADDING, HISTORY_DETAILS_PANEL_WIDTH,
    LABEL_PADDING as InputSuggestionsLabelPadding,
};
use crate::themes::theme::WarpTheme;

enum SuggestionsResizeConfig {
    WidthAndHeight,
    HeightOnly,
}

impl Input {
    pub(super) fn render_history_up_menu(
        &self,
        appearance: &Appearance,
        menu_positioning: MenuPositioning,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let radius = Radius::Pixels(6.);

        let corner_radius = match menu_positioning {
            MenuPositioning::AboveInputBox => CornerRadius::with_top(radius),
            MenuPositioning::BelowInputBox => CornerRadius::with_bottom(radius),
        };

        let input_suggestions_border = 2.0;
        let margin =
            *TERMINAL_VIEW_PADDING_LEFT - input_suggestions_border - InputSuggestionsLabelPadding;

        let content = ChildView::new(&self.input_suggestions).finish();

        SizeConstraintSwitch::new(
            Flex::row()
                .with_children([
                    Shrinkable::new(
                        1.,
                        self.render_suggestions_container(
                            margin,
                            corner_radius,
                            theme,
                            SuggestionsResizeConfig::HeightOnly,
                            menu_positioning,
                            content,
                        ),
                    )
                    .finish(),
                    ConstrainedBox::new(Empty::new().finish())
                        .with_width(
                            HISTORY_DETAILS_PANEL_WIDTH
                                + DETAILS_PANEL_MARGIN * 2.
                                + DETAILS_PANEL_PADDING * 2.,
                        )
                        .finish(),
                ])
                .finish(),
            vec![(
                SizeConstraintCondition::WidthLessThan(HISTORY_DETAILS_VIEW_WIDTH_REQUIREMENT),
                self.render_suggestions_container(
                    margin,
                    corner_radius,
                    theme,
                    SuggestionsResizeConfig::HeightOnly,
                    menu_positioning,
                    ChildView::new(&self.input_suggestions).finish(),
                ),
            )],
        )
        .finish()
    }

    pub(super) fn render_completion_suggestions_menu(
        &self,
        appearance: &Appearance,
        menu_positioning: MenuPositioning,
    ) -> Box<dyn Element> {
        let theme = appearance.theme();
        let corner_radius = CornerRadius::with_all(Radius::Pixels(6.));

        let content = ChildView::new(&self.input_suggestions).finish();

        self.render_suggestions_container(
            0.,
            corner_radius,
            theme,
            SuggestionsResizeConfig::WidthAndHeight,
            menu_positioning,
            content,
        )
    }

    fn render_suggestions_container(
        &self,
        margin: f32,
        corner_radius: CornerRadius,
        theme: &WarpTheme,
        resize_config: SuggestionsResizeConfig,
        menu_positioning: MenuPositioning,
        content: Box<dyn Element>,
    ) -> Box<dyn Element> {
        // Inner container for the visual menu content (background, border, shadow)
        // This is wrapped by Resizable so the resize handles align with the border.
        let inner_container = Container::new(content)
            .with_background(theme.surface_2())
            .with_corner_radius(corner_radius)
            .with_drop_shadow(DropShadow::default())
            .with_border(Border::all(1.0).with_border_fill(theme.outline()))
            .finish();

        // Apply width resizing based on config
        let horizontal_resizable = match resize_config {
            SuggestionsResizeConfig::WidthAndHeight => {
                let width_handle_for_end = self.completions_menu_resizable_width.clone();
                Resizable::new(
                    self.completions_menu_resizable_width.clone(),
                    inner_container,
                )
                .with_dragbar_side(DragBarSide::Right)
                .with_dragbar_offset(7.0)
                .with_bounds_callback(Box::new(|window_size| (200.0, window_size.x().max(200.0))))
                .on_resize(move |ctx, _| {
                    ctx.notify();
                })
                .on_end_resizing(move |ctx, _| {
                    let new_width = width_handle_for_end
                        .lock()
                        .expect("width handle lock poisoned")
                        .size();
                    ctx.dispatch_typed_action(InputAction::UpdateCompletionsMenuWidth(new_width));
                })
                .finish()
            }
            SuggestionsResizeConfig::HeightOnly => inner_container,
        };

        // Apply height resizing based on config
        let resizable_element = match resize_config {
            SuggestionsResizeConfig::WidthAndHeight | SuggestionsResizeConfig::HeightOnly => {
                let dragbar_side = match menu_positioning {
                    MenuPositioning::AboveInputBox => DragBarSide::Top,
                    MenuPositioning::BelowInputBox => DragBarSide::Bottom,
                };

                let height_handle_for_end = self.completions_menu_resizable_height.clone();
                Resizable::new(
                    self.completions_menu_resizable_height.clone(),
                    horizontal_resizable,
                )
                .with_dragbar_side(dragbar_side)
                .with_bounds_callback(Box::new(|window_size| (100.0, window_size.y().max(100.0))))
                .on_resize(move |ctx, _| {
                    ctx.notify();
                })
                .on_end_resizing(move |ctx, _| {
                    let new_height = height_handle_for_end
                        .lock()
                        .expect("height handle lock poisoned")
                        .size();
                    ctx.dispatch_typed_action(InputAction::UpdateCompletionsMenuHeight(new_height));
                })
                .finish()
            }
        };

        Container::new(resizable_element)
            .with_margin_top(12.0)
            .with_margin_left(margin)
            .with_margin_right(margin)
            .finish()
    }

}
