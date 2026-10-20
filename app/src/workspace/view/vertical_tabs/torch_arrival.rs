use pathfinder_geometry::vector::Vector2F;
use warpui::elements::{Element, Point, SizeConstraint};
use warpui::event::DispatchedEvent;
use warpui::{
    AfterLayoutContext, AppContext, ClipBounds, EventContext, LayoutContext, PaintContext,
};

/// Reveals a full-size row without squeezing its text or hit targets as space opens.
pub(super) struct SessionArrival {
    child: Box<dyn Element>,
    progress: f32,
    size: Option<Vector2F>,
    origin: Option<Point>,
}

impl SessionArrival {
    pub(super) fn new(child: Box<dyn Element>, progress: f32) -> Self {
        Self {
            child,
            progress,
            size: None,
            origin: None,
        }
    }
}

impl Element for SessionArrival {
    fn layout(
        &mut self,
        constraint: SizeConstraint,
        ctx: &mut LayoutContext,
        app: &AppContext,
    ) -> Vector2F {
        let full_size = self.child.layout(constraint, ctx, app);
        let size = Vector2F::new(full_size.x(), full_size.y() * self.progress);
        self.size = Some(size);
        size
    }

    fn after_layout(&mut self, ctx: &mut AfterLayoutContext, app: &AppContext) {
        self.child.after_layout(ctx, app);
    }

    fn paint(&mut self, origin: Vector2F, ctx: &mut PaintContext, app: &AppContext) {
        let point = Point::from_vec2f(origin, ctx.scene.z_index());
        self.origin = Some(point);
        if let Some(bounds) = ctx.scene.visible_rect(point, self.size.unwrap()) {
            ctx.scene.start_layer(ClipBounds::BoundedBy(bounds));
            self.child.paint(origin, ctx, app);
            ctx.scene.stop_layer();
        }
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
