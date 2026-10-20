use std::time::Duration;

use instant::Instant;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{Vector2F, vec2f};
use warpui::elements::{Element, Point, SizeConstraint};
use warpui::event::DispatchedEvent;
use warpui::{
    AfterLayoutContext, AppContext, ClipBounds, EventContext, LayoutContext, PaintContext,
};

/// Delay between animation frames; short enough for 120 Hz displays.
const FRAME_INTERVAL: Duration = Duration::from_millis(8);
/// Horizontal distance a row travels as it enters or leaves.
const SLIDE_DISTANCE: f32 = 12.;
/// Gap between successive motion-blur copies at full speed.
const BLUR_SPACING: f32 = 1.6;
/// Relative opacity of the motion-blur copies on each side of the row, nearest first.
const BLUR_WEIGHTS: [f32; 3] = [0.28, 0.16, 0.07];
/// How much the sharp copy dims at full speed, so the blur doesn't read as bolder text.
const BLUR_DIMMING: f32 = 0.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Motion {
    /// Slides the row in from the left.
    Arrival,
    /// Fades the row in place, for Reduce Motion.
    ReducedArrival,
    /// Slides the row out to the left, then closes its space once it has gone.
    Departure,
}

/// The state of a [`Motion`] at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct MotionFrame {
    /// Fraction of the child's height the row occupies in the list.
    pub(super) height: f32,
    /// Horizontal displacement of the content, in pixels.
    pub(super) offset: f32,
    pub(super) opacity: f32,
    /// Motion-blur strength from 0 to 1, proportional to the row's speed.
    pub(super) blur: f32,
}

impl MotionFrame {
    pub(super) const SETTLED: Self = Self {
        height: 1.,
        offset: 0.,
        opacity: 1.,
        blur: 0.,
    };

    const GONE: Self = Self {
        height: 0.,
        offset: -SLIDE_DISTANCE,
        opacity: 0.,
        blur: 0.,
    };
}

/// Fraction of a departure spent sliding out; the remainder closes the empty space.
const DEPARTURE_SLIDE: f32 = 0.6;

impl Motion {
    pub(super) fn duration(self) -> Duration {
        match self {
            Motion::Arrival => Duration::from_millis(340),
            Motion::ReducedArrival => Duration::from_millis(160),
            Motion::Departure => Duration::from_millis(300),
        }
    }

    pub(super) fn frame(self, elapsed: Duration) -> MotionFrame {
        let t = elapsed.as_secs_f32() / self.duration().as_secs_f32();
        match self {
            Motion::Arrival | Motion::ReducedArrival if t >= 1. => MotionFrame::SETTLED,
            Motion::Departure if t >= 1. => MotionFrame::GONE,
            // Cubic ease-out: the blur follows its speed, (1 - t)², fading as the row settles.
            Motion::Arrival => MotionFrame {
                height: 1.,
                offset: -SLIDE_DISTANCE * (1. - ease_out(t, 3)),
                opacity: ease_out(segment(t, 0., 0.6), 2),
                blur: (1. - t).powi(2),
            },
            Motion::ReducedArrival => MotionFrame {
                opacity: ease_out(t, 2),
                ..MotionFrame::SETTLED
            },
            // Quadratic ease-in, so the blur grows with its speed; the space only starts closing
            // once the content has fully faded, so the row never appears to move vertically.
            Motion::Departure => {
                let slide = segment(t, 0., DEPARTURE_SLIDE);
                MotionFrame {
                    height: 1. - ease_in_out_cubic(segment(t, DEPARTURE_SLIDE, 1.)),
                    offset: -SLIDE_DISTANCE * ease_in(slide, 2),
                    opacity: 1. - ease_in(slide, 2),
                    blur: slide,
                }
            }
        }
    }
}

fn segment(t: f32, start: f32, end: f32) -> f32 {
    ((t - start) / (end - start)).clamp(0., 1.)
}

fn ease_in(t: f32, power: i32) -> f32 {
    t.powi(power)
}

fn ease_out(t: f32, power: i32) -> f32 {
    1. - (1. - t).powi(power)
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4. * t.powi(3)
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// Plays a [`Motion`] on a sidebar row, repainting itself until the motion ends. The child keeps
/// its full size throughout, so its text and hit targets never squeeze.
pub(super) struct SessionMotion {
    child: Box<dyn Element>,
    motion: Motion,
    started: Instant,
    frame: MotionFrame,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SessionMotion {
    pub(super) fn new(child: Box<dyn Element>, motion: Motion, started: Instant) -> Self {
        Self {
            child,
            motion,
            started,
            frame: MotionFrame::SETTLED,
            size: None,
            origin: None,
        }
    }
}

impl Element for SessionMotion {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let full_size = self.child.layout(constraint, ctx, app);
        self.frame = self.motion.frame(self.started.elapsed());
        let size = vec2f(full_size.x(), full_size.y() * self.frame.height);
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        if self.started.elapsed() < self.motion.duration() {
            ctx.repaint_after(FRAME_INTERVAL);
        }
        let frame = self.frame;
        if frame == MotionFrame::SETTLED {
            self.child.paint(origin, ctx, app);
            return;
        }
        let Some(size) = self.size else {
            return;
        };
        if size.y() <= 0. || frame.opacity <= 0. {
            return;
        }
        let content_origin = origin + vec2f(frame.offset, 0.);
        let overhang = SLIDE_DISTANCE + BLUR_SPACING * BLUR_WEIGHTS.len() as f32;
        let slot = RectF::new(
            origin - vec2f(overhang, 0.),
            size + vec2f(2. * overhang, 0.),
        );
        // Blur copies are painted first so the sharp copy, painted last, owns hit testing.
        for (copy, weight) in BLUR_WEIGHTS.iter().enumerate().rev() {
            let opacity = frame.opacity * weight * frame.blur;
            if opacity < 0.01 {
                continue;
            }
            let shift = (copy + 1) as f32 * BLUR_SPACING * frame.blur;
            for side in [-1., 1.] {
                ctx.scene
                    .start_layer_with_opacity(ClipBounds::BoundedByActiveLayerAnd(slot), opacity);
                ctx.scene.set_active_layer_click_through();
                self.child
                    .paint(content_origin + vec2f(side * shift, 0.), ctx, app);
                ctx.scene.stop_layer();
            }
        }
        ctx.scene.start_layer_with_opacity(
            ClipBounds::BoundedByActiveLayerAnd(slot),
            frame.opacity * (1. - BLUR_DIMMING * frame.blur),
        );
        self.child.paint(content_origin, ctx, app);
        ctx.scene.stop_layer();
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        event: &DispatchedEvent,
        ctx: &mut EventContext,
        app: &AppContext,
    ) -> bool {
        self.child.origin().is_some() && self.child.dispatch_event(event, ctx, app)
    }
}
