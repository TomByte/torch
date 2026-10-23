use pathfinder_geometry::vector::vec2f;

use super::*;
use crate::rendering;

#[test]
fn test_hit_rect_recording() {
    let mut scene = Scene::new(1., rendering::Config::default());
    assert_eq!(ZIndex::new(0), scene.z_index());

    scene.draw_rect_with_hit_recording(RectF::new(vec2f(0., 0.), vec2f(100., 100.)));
    assert_eq!(ZIndex::new(0), scene.z_index());
    assert!(!scene.is_covered(Point::new(10., 10., ZIndex::new(0))));

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.draw_rect_with_hit_recording(RectF::new(vec2f(50., 50.), vec2f(100., 100.)));
    assert_eq!(ZIndex::new(1), scene.z_index());
    assert!(!scene.is_covered(Point::new(10., 10., ZIndex::new(0))));
    assert!(scene.is_covered(Point::new(60., 60., ZIndex::new(0))));

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.draw_rect_with_hit_recording(RectF::new(vec2f(0., 0.), vec2f(100., 100.)));
    assert_eq!(ZIndex::new(2), scene.z_index());
    assert!(scene.is_covered(Point::new(10., 10., ZIndex::new(0))));
    assert!(scene.is_covered(Point::new(60., 60., ZIndex::new(1))));
}

#[test]
fn test_nested_clip_bounds_with_intersection() {
    let mut scene = Scene::new(1., rendering::Config::default());

    let bounds1 = RectF::new(Vector2F::zero(), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedBy(bounds1));

    let bounds2 = RectF::new(Vector2F::new(5., 5.), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedByActiveLayerAnd(bounds2));

    assert_eq!(
        scene.active_layer().clip_bounds,
        Some(RectF::new(Vector2F::new(5., 5.), Vector2F::new(5., 5.)))
    );
}

#[test]
fn test_nested_clip_bounds_with_no_intersection() {
    let mut scene = Scene::new(1., rendering::Config::default());

    let bounds1 = RectF::new(Vector2F::zero(), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedBy(bounds1));

    let bounds2 = RectF::new(Vector2F::new(100., 100.), Vector2F::new(10., 10.));
    scene.start_layer(ClipBounds::BoundedByActiveLayerAnd(bounds2));

    // We should have explicit bounds for this layer.  (None represents a lack
    // of clipping, not clipping they layer down to nothingness.)
    assert!(scene.active_layer().clip_bounds.is_some());
    // The clip bounds should have an area of zero.
    assert!(scene.active_layer().clip_bounds.unwrap().is_empty());
}

#[test]
fn test_click_through_layer_does_not_cover_lower_layers() {
    let mut scene = Scene::new(1., rendering::Config::default());

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.set_active_layer_click_through();
    scene.draw_rect_with_hit_recording(RectF::new(vec2f(0., 0.), vec2f(100., 100.)));

    assert!(!scene.is_covered(Point::new(10., 10., ZIndex::new(0))));
}

#[test]
fn layer_opacity_fades_every_primitive_and_compounds_when_nested() {
    let mut scene = Scene::new(1., rendering::Config::default());
    let bounds = RectF::new(vec2f(0., 0.), vec2f(10., 10.));
    let color = ColorU::new(10, 20, 30, 200);

    scene.start_layer_with_opacity(ClipBounds::ActiveLayer, 0.5);
    scene
        .draw_rect_with_hit_recording(bounds)
        .with_background(Fill::Gradient {
            start: vec2f(0., 0.),
            end: vec2f(0., 1.),
            start_color: color,
            end_color: ColorU::new(0, 0, 0, 100),
        })
        .with_border(Border {
            width: 1.,
            color: Fill::Solid(color),
            ..Default::default()
        })
        .with_drop_shadow(DropShadow::new_with_standard_offset_and_spread(color));
    scene.draw_glyph(Vector2F::zero(), 0, FontId(0), 12., color);
    scene.start_layer_with_opacity(ClipBounds::ActiveLayer, 0.5);
    scene.draw_glyph(Vector2F::zero(), 0, FontId(0), 12., color);
    scene.stop_layer();
    scene.stop_layer();

    let layers = scene.layers().collect::<Vec<_>>();
    let rect = &layers[1].rects[0];
    let Fill::Gradient {
        start_color,
        end_color,
        ..
    } = rect.background
    else {
        panic!("gradient background should stay a gradient");
    };
    assert_eq!((start_color.a, end_color.a), (100, 50));
    assert_eq!(rect.border.color, Fill::Solid(ColorU::new(10, 20, 30, 100)));
    assert_eq!(rect.drop_shadow.unwrap().color.a, 100);
    assert_eq!(layers[1].glyphs[0].color, ColorU::new(10, 20, 30, 100));
    assert_eq!(layers[2].glyphs[0].color.a, 50);
}

#[test]
fn layers_without_opacity_keep_their_colors() {
    let mut scene = Scene::new(1., rendering::Config::default());
    let color = ColorU::new(10, 20, 30, 200);

    scene.start_layer(ClipBounds::ActiveLayer);
    scene.draw_glyph(Vector2F::zero(), 0, FontId(0), 12., color);
    scene.stop_layer();

    assert_eq!(scene.layers().nth(1).unwrap().glyphs[0].color, color);
}
