use super::{SurfaceDestination, surface_unavailable_reason};
use crate::features::FeatureFlag;

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

#[test]
fn agent_management_surface_reports_feature_flag_unavailable() {
    let flag_guard = FeatureFlag::AgentManagementView.override_enabled(false);
    torchui::App::test((), |mut app| async move {
        assert_eq!(
            app.update(|ctx| {
                surface_unavailable_reason(SurfaceDestination::AgentManagement, ctx)
            }),
            Some("agent management is unavailable or disabled")
        );
    });
    drop(flag_guard);
}
