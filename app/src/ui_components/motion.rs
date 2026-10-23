//! Motion shared across Torch's surfaces: the system's motion preference, and how surfaces that
//! float over the workspace, such as the command palette, menus and toasts, come and go.

use std::time::Duration;

use instant::Instant;
use pathfinder_geometry::vector::{Vector2F, vec2f};
use torchui::elements::{Element, Point, SizeConstraint};
use torchui::event::DispatchedEvent;
use torchui::{
    AfterLayoutContext, AppContext, ClipBounds, EventContext, LayoutContext, PaintContext,
};

/// Delay between animation frames; short enough for 120 Hz displays.
pub(crate) const FRAME_INTERVAL: Duration = Duration::from_millis(8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MotionPreference {
    Full,
    Reduced,
}

/// The system motion preference, or `None` where surfaces change immediately.
pub(crate) fn motion_preference() -> Option<MotionPreference> {
    #[cfg(target_os = "macos")]
    {
        Some(
            if objc2_app_kit::NSWorkspace::sharedWorkspace()
                .accessibilityDisplayShouldReduceMotion()
            {
                MotionPreference::Reduced
            } else {
                MotionPreference::Full
            },
        )
    }
    // Keep changes immediate until the other backends expose their system motion preference.
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Exponential ease-out: quick to respond, slow to settle.
pub(crate) fn ease_out_expo(t: f32) -> f32 {
    if t >= 1. {
        1.
    } else {
        1. - 2_f32.powf(-10. * t)
    }
}

/// How a surface that floats over the workspace comes or goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Reveal {
    /// A panel, such as the command palette, settling down into place.
    Arrive,
    /// A small surface, such as a menu, appearing where it was summoned.
    Pop,
    /// A control replacing another in the same place.
    Swap,
    /// A dismissed panel lifting away; it takes no input as it goes.
    Leave,
}

/// A [`Reveal`] at one instant.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct RevealFrame {
    pub(crate) opacity: f32,
    /// Vertical displacement, in logical pixels.
    pub(crate) offset: f32,
}

impl Reveal {
    pub(crate) fn duration(self) -> Duration {
        match self {
            Reveal::Arrive => Duration::from_millis(220),
            Reveal::Pop => Duration::from_millis(150),
            Reveal::Swap => Duration::from_millis(110),
            Reveal::Leave => Duration::from_millis(130),
        }
    }

    /// How far the surface travels with full motion. Reduced motion keeps only the fade.
    fn travel(self) -> f32 {
        match self {
            Reveal::Arrive => 8.,
            Reveal::Pop | Reveal::Leave => 4.,
            Reveal::Swap => 0.,
        }
    }

    pub(crate) fn frame(self, elapsed: Duration, preference: MotionPreference) -> RevealFrame {
        let t = (elapsed.as_secs_f32() / self.duration().as_secs_f32()).min(1.);
        let travel = match preference {
            MotionPreference::Full => self.travel(),
            MotionPreference::Reduced => 0.,
        };
        match self {
            // The position settles exponentially while the fade trails it slightly.
            Reveal::Arrive | Reveal::Pop | Reveal::Swap => RevealFrame {
                opacity: 1. - (1. - t).powi(2),
                offset: -travel * (1. - ease_out_expo(t)),
            },
            Reveal::Leave => RevealFrame {
                opacity: 1. - t * t,
                offset: -travel * t * t,
            },
        }
    }

    /// Whether this transition, started at `started`, has finished.
    pub(crate) fn is_finished(self, started: Instant) -> bool {
        motion_preference().is_none() || started.elapsed() >= self.duration()
    }
}

/// Plays a [`Reveal`] on its child, repainting itself until the transition ends.
pub(crate) struct Revealing {
    child: Box<dyn Element>,
    reveal: Reveal,
    started: Instant,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl Revealing {
    pub(crate) fn new(child: Box<dyn Element>, reveal: Reveal, started: Instant) -> Self {
        Self {
            child,
            reveal,
            started,
            size: None,
            origin: None,
        }
    }
}

impl Element for Revealing {
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
        let elapsed = self.started.elapsed();
        let preference = motion_preference().filter(|_| elapsed < self.reveal.duration());
        let Some(preference) = preference else {
            if self.reveal != Reveal::Leave {
                self.child.paint(origin, ctx, app);
            }
            return;
        };
        ctx.repaint_after(FRAME_INTERVAL);
        let frame = self.reveal.frame(elapsed, preference);
        ctx.scene
            .start_layer_with_opacity(ClipBounds::ActiveLayer, frame.opacity);
        if self.reveal == Reveal::Leave {
            ctx.scene.set_active_layer_click_through();
        }
        self.child.paint(origin + vec2f(0., frame.offset), ctx, app);
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
        self.reveal != Reveal::Leave && self.child.dispatch_event(event, ctx, app)
    }
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
