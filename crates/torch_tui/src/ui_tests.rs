use torchui_core::App;
use torchui_core::elements::tui::{TuiBufferExt, TuiElement, TuiRect, TuiText};
use torchui_core::presenter::tui::TuiPresenter;

use super::{compact_footer_path, horizontally_centered};

#[test]
fn compact_footer_path_preserves_short_paths() {
    assert_eq!(compact_footer_path("/erica/project"), "/erica/project");
}

#[test]
fn horizontally_centered_balances_available_space() {
    App::test((), |app| async move {
        app.read(|ctx| {
            let mut presenter = TuiPresenter::new();
            let frame = presenter.present_element(
                horizontally_centered(TuiText::new("center").finish()),
                TuiRect::new(0, 0, 20, 1),
                ctx,
            );
            let line = &frame.buffer.to_lines()[0];
            let left = line.find("center").expect("centered text renders");
            let right = 20 - left - "center".len();
            assert!(left.abs_diff(right) <= 1, "{line:?}");
        });
    });
}

#[test]
fn compact_footer_path_elides_middle_components() {
    assert_eq!(compact_footer_path("~/Documents/GitHub/warp"), "~/…/warp");
    assert_eq!(compact_footer_path("/long/path/to/project"), "/…/project");
    assert_eq!(
        compact_footer_path(r"C:\Users\erica\project"),
        r"C:\…\project"
    );
}
