//! Shadows for surfaces that float above the workspace, such as the command palette and toasts.

use pathfinder_color::ColorU;
use pathfinder_geometry::vector::vec2f;
use torch_core::ui::appearance::Appearance;
use torch_core::ui::theme::ColorScheme;
use torchui::elements::DropShadow;

/// Room around a shadow's shape for its blur to fade out, as a multiple of the blur. The
/// renderer cuts a shadow off at its shape grown by this room, so a smaller one shows a hard edge.
const BLUR_ROOM: f32 = 3.;

/// A tight shadow that seats the surface's edge, then a broad, soft one whose reach grows with
/// `lift`. Dark themes need deeper shadows for the same lift.
pub(crate) fn floating_shadows(appearance: &Appearance, lift: f32) -> [DropShadow; 2] {
    let (contact, ambient) =
        if appearance.theme().inferred_color_scheme() == ColorScheme::DarkOnLight {
            (0x1A1F361A, 0x1A1F3633)
        } else {
            (0x00000066, 0x0000008C)
        };
    [
        soft_shadow(ColorU::from_u32(contact), 1., 1.5),
        soft_shadow(ColorU::from_u32(ambient), lift * 0.75, lift),
    ]
}

/// A shadow `drop` pixels below its shape, blurred by `blur`, with room to fade out completely.
pub(crate) fn soft_shadow(color: ColorU, drop: f32, blur: f32) -> DropShadow {
    DropShadow {
        color,
        offset: vec2f(0., drop),
        blur_radius: blur,
        spread_radius: blur * BLUR_ROOM,
    }
}
