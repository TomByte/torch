use super::{SurfaceDestination, surface_unavailable_reason};

#[cfg(feature = "local_only")]
#[test]
fn terminal_only_editor_surfaces_are_unavailable() {
    torchui::App::test((), |mut app| async move {
        app.update(|ctx| {
            for destination in [
                SurfaceDestination::CodeReview,
                SurfaceDestination::RightPanel,
                SurfaceDestination::ProjectExplorer,
                SurfaceDestination::GlobalSearch,
                SurfaceDestination::LeftPanel,
            ] {
                assert!(surface_unavailable_reason(destination, ctx).is_some());
            }
            assert!(surface_unavailable_reason(SurfaceDestination::CommandPalette, ctx).is_none());
        });
    });
}
