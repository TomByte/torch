use std::f32::consts::PI;
use std::sync::LazyLock;
use std::time::Duration;

use instant::Instant;
use pathfinder_color::ColorU;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::{Vector2F, vec2f};
use torchui::elements::{Border, CornerRadius, Element, Point, Radius, SizeConstraint};
use torchui::event::DispatchedEvent;
use torchui::{
    AfterLayoutContext, AppContext, ClipBounds, EventContext, LayoutContext, PaintContext,
};

use crate::ui_components::elevation::soft_shadow;
use crate::ui_components::motion::{FRAME_INTERVAL, MotionPreference, ease_out_expo};
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

/// Delay between frames of the working pulse. It loops for as long as an agent works, so it
/// redraws at a gentler rate than the one-off row motions.
const PULSE_FRAME_INTERVAL: Duration = Duration::from_millis(33);
const PULSE_PERIOD: Duration = Duration::from_millis(1600);
const PULSE_DOT_RADIUS: f32 = 3.5;
const PULSE_RING_RADIUS: f32 = 7.;
const PULSE_RING_WIDTH: f32 = 1.5;
/// Opacity of the ring that stays in place while the moving ring closes in.
const PULSE_RESTING_RING_OPACITY: f32 = 0.3;

/// Shared by every pulse so all working sessions breathe in step, however often rows rebuild.
static PULSE_EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Peak opacity of the ring that closes in on the dot, reached halfway through each cycle.
const PULSE_PEAK_OPACITY: f32 = 0.8;

/// Radius and opacity of the ring that closes in on the dot, at `t` through one cycle. It fades in
/// from nothing and out to nothing, so one cycle hands over to the next without a visible reset.
pub(super) fn pulse_ring(t: f32) -> (f32, f32) {
    let progress = (1. - (PI * t).cos()) / 2.;
    (
        PULSE_RING_RADIUS - (PULSE_RING_RADIUS - PULSE_DOT_RADIUS) * progress,
        PULSE_PEAK_OPACITY * (PI * t).sin().powi(2),
    )
}

/// An agent-is-working indicator: a dot with a ring that repeatedly closes in on it and fades.
/// With `animated` false it rests as a dot inside a faint ring.
pub(super) struct WorkingPulse {
    color: ColorU,
    animated: bool,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl WorkingPulse {
    pub(super) fn new(color: ColorU, animated: bool) -> Self {
        Self {
            color,
            animated,
            size: None,
            origin: None,
        }
    }

    fn draw_circle(
        &self,
        ctx: &mut PaintContext,
        center: Vector2F,
        radius: f32,
        style: CircleStyle,
    ) {
        let rect = RectF::new(center - vec2f(radius, radius), vec2f(radius, radius) * 2.);
        let circle = ctx.scene.draw_rect_without_hit_recording(rect);
        circle.with_corner_radius(CornerRadius::with_all(Radius::Pixels(radius)));
        match style {
            CircleStyle::Filled(opacity) => {
                circle.with_background(with_opacity(self.color, opacity));
            }
            CircleStyle::Outlined(opacity) => {
                circle.with_border(
                    Border::all(PULSE_RING_WIDTH)
                        .with_border_color(with_opacity(self.color, opacity)),
                );
            }
        }
    }
}

/// How a circle is drawn, with its opacity.
enum CircleStyle {
    Filled(f32),
    Outlined(f32),
}

fn with_opacity(color: ColorU, opacity: f32) -> ColorU {
    ColorU::new(
        color.r,
        color.g,
        color.b,
        (f32::from(color.a) * opacity).round() as u8,
    )
}

impl Element for WorkingPulse {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        _ctx: &mut LayoutContext,
        _app: &AppContext,
    ) -> Vector2F {
        let size = constraint.max;
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, _ctx: &mut AfterLayoutContext, _app: &AppContext) {}

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, _app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        let Some(size) = self.size else {
            return;
        };
        let center = origin + size / 2.;
        self.draw_circle(
            ctx,
            center,
            PULSE_RING_RADIUS,
            CircleStyle::Outlined(PULSE_RESTING_RING_OPACITY),
        );
        if self.animated {
            ctx.repaint_after(PULSE_FRAME_INTERVAL);
            let period = PULSE_PERIOD.as_secs_f32();
            let t = PULSE_EPOCH.elapsed().as_secs_f32() % period / period;
            let (radius, opacity) = pulse_ring(t);
            self.draw_circle(ctx, center, radius, CircleStyle::Outlined(opacity));
        }
        self.draw_circle(ctx, center, PULSE_DOT_RADIUS, CircleStyle::Filled(1.));
    }

    fn size(&self) -> Option<Vector2F> {
        self.size
    }

    fn origin(&self) -> Option<Point> {
        self.origin
    }

    fn dispatch_event(
        &mut self,
        _event: &DispatchedEvent,
        _ctx: &mut EventContext,
        _app: &AppContext,
    ) -> bool {
        false
    }
}

/// How long a row's hover tint takes to appear, and to clear once the pointer leaves.
pub(super) const HOVER_IN: Duration = Duration::from_millis(120);
pub(super) const HOVER_OUT: Duration = Duration::from_millis(90);

/// A value easing towards a target. Retargeting starts from wherever it currently is, so an
/// interrupted fade never jumps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fade {
    from: f32,
    to: f32,
    /// When it started towards `to`, or `None` if it has been there all along.
    since: Option<Instant>,
    duration: Duration,
}

impl Fade {
    pub(super) fn settled(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            since: None,
            duration: Duration::ZERO,
        }
    }

    pub(super) fn target(self) -> f32 {
        self.to
    }

    /// When the fade started towards its target, or `None` if it has always been there.
    pub(super) fn since(self) -> Option<Instant> {
        self.since
    }

    pub(super) fn retarget(self, to: f32, duration: Duration, now: Instant) -> Self {
        Self {
            from: self.value(now),
            to,
            since: Some(now),
            duration,
        }
    }

    pub(super) fn value(self, now: Instant) -> f32 {
        let Some(since) = self.since else {
            return self.to;
        };
        let elapsed = now.saturating_duration_since(since);
        if elapsed >= self.duration {
            return self.to;
        }
        let t = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        self.from + (self.to - self.from) * ease_out(t, 2)
    }

    pub(super) fn is_settled(self, now: Instant) -> bool {
        self.since
            .is_none_or(|since| now.saturating_duration_since(since) >= self.duration)
    }
}

/// How long the active card takes to reach a newly selected row.
pub(super) const CARD_GLIDE: Duration = Duration::from_millis(220);
/// How long the active card takes to fade in where it cannot glide.
const CARD_FADE: Duration = Duration::from_millis(160);
/// The farthest the card glides. A previous row further away is treated as being this far, so
/// distant switches still read as movement without sweeping the whole list.
pub(super) const CARD_MAX_GLIDE: f32 = 180.;

/// How the active session's card reaches its row after the selection changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum CardEntrance {
    /// The card has been on this row since it was first drawn.
    Settled,
    /// Glides in from the window-space top edge of the previously active row.
    Glide { from_y: f32, since: Instant },
    /// Fades in place, where the previous row was not on screen or motion is reduced.
    Fade { since: Instant },
}

/// The active card at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct CardFrame {
    /// Vertical displacement from the card's row, in pixels.
    pub(super) offset: f32,
    pub(super) opacity: f32,
}

impl CardFrame {
    pub(super) const SETTLED: Self = Self {
        offset: 0.,
        opacity: 1.,
    };
}

impl CardEntrance {
    /// The entrance for a card leaving a row whose window-space top edge was at `from_y`, if that
    /// row was on screen.
    pub(super) fn after_switch(
        from_y: Option<f32>,
        preference: Option<MotionPreference>,
        now: Instant,
    ) -> Self {
        match (preference, from_y) {
            (None, _) => CardEntrance::Settled,
            (Some(MotionPreference::Full), Some(from_y)) => {
                CardEntrance::Glide { from_y, since: now }
            }
            (Some(MotionPreference::Full), None) | (Some(MotionPreference::Reduced), _) => {
                CardEntrance::Fade { since: now }
            }
        }
    }

    /// The card's frame `elapsed` into its entrance, for a row whose top edge is at `row_y`.
    pub(super) fn frame(self, row_y: f32, elapsed: Duration) -> CardFrame {
        match self {
            CardEntrance::Settled => CardFrame::SETTLED,
            CardEntrance::Glide { from_y, .. } => {
                let t = (elapsed.as_secs_f32() / CARD_GLIDE.as_secs_f32()).min(1.);
                let distance = (from_y - row_y).clamp(-CARD_MAX_GLIDE, CARD_MAX_GLIDE);
                CardFrame {
                    offset: distance * (1. - ease_out_expo(t)),
                    opacity: 1.,
                }
            }
            CardEntrance::Fade { .. } => {
                let t = (elapsed.as_secs_f32() / CARD_FADE.as_secs_f32()).min(1.);
                CardFrame {
                    offset: 0.,
                    opacity: ease_out(t, 2),
                }
            }
        }
    }

    fn since(self) -> Option<Instant> {
        match self {
            CardEntrance::Settled => None,
            CardEntrance::Glide { since, .. } | CardEntrance::Fade { since } => Some(since),
        }
    }

    fn duration(self) -> Duration {
        match self {
            CardEntrance::Settled => Duration::ZERO,
            CardEntrance::Glide { .. } => CARD_GLIDE,
            CardEntrance::Fade { .. } => CARD_FADE,
        }
    }
}

pub(super) const ROW_RADIUS: f32 = 8.;

/// The colors of a row's fills.
#[derive(Clone, Copy, Debug)]
pub(super) struct RowFills {
    pub(super) hover: ColorU,
    pub(super) selection: ColorU,
    pub(super) card_edge: ColorU,
    pub(super) card_shadow: ColorU,
}

/// Paints the fills behind a session row: its hover tint, its multi-selection fill and, for the
/// active session, a raised card. The card is drawn by its destination row; a gliding card passes
/// over the rows between, below their text and icons.
pub(super) struct RowSurface {
    child: Box<dyn Element>,
    fills: RowFills,
    hover: Fade,
    multi_selected: bool,
    card: Option<CardEntrance>,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl RowSurface {
    pub(super) fn new(
        child: Box<dyn Element>,
        fills: RowFills,
        hover: Fade,
        multi_selected: bool,
        card: Option<CardEntrance>,
    ) -> Self {
        Self {
            child,
            fills,
            hover,
            multi_selected,
            card,
            size: None,
            origin: None,
        }
    }
}

impl Element for RowSurface {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let size = self.child.layout(constraint, ctx, app);
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        self.origin = Some(Point::from_vec2f(origin, ctx.scene.z_index()));
        if let Some(size) = self.size {
            let now = Instant::now();
            let rect = RectF::new(origin, size);
            let radius = CornerRadius::with_all(Radius::Pixels(ROW_RADIUS));
            let hover = self.hover.value(now);
            if hover > 0. {
                ctx.scene
                    .draw_rect_without_hit_recording(rect)
                    .with_corner_radius(radius)
                    .with_background(with_opacity(self.fills.hover, hover));
            }
            if self.multi_selected {
                ctx.scene
                    .draw_rect_without_hit_recording(rect)
                    .with_corner_radius(radius)
                    .with_background(self.fills.selection);
            }
            let mut animating = !self.hover.is_settled(now);
            if let Some(card) = self.card {
                let elapsed = card
                    .since()
                    .map_or(Duration::MAX, |since| now.saturating_duration_since(since));
                animating |= elapsed < card.duration();
                let frame = card.frame(origin.y(), elapsed);
                if frame.opacity > 0. {
                    ctx.scene
                        .draw_rect_without_hit_recording(rect + vec2f(0., frame.offset))
                        .with_corner_radius(radius)
                        .with_background(with_opacity(self.fills.selection, frame.opacity))
                        .with_border(
                            Border::all(1.).with_border_color(with_opacity(
                                self.fills.card_edge,
                                frame.opacity,
                            )),
                        )
                        .with_drop_shadow(soft_shadow(
                            with_opacity(self.fills.card_shadow, frame.opacity),
                            1.,
                            2.,
                        ));
                }
            }
            if animating {
                ctx.repaint_after(FRAME_INTERVAL);
            }
        }
        self.child.paint(origin, ctx, app);
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
        self.child.dispatch_event(event, ctx, app)
    }
}
