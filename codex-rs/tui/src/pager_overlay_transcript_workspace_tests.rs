use super::TranscriptOverlay;
use super::transcript_workspace::TranscriptWorkspaceLayout;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use ratatui::layout::Rect;

#[test]
fn workspace_layout_keeps_the_composer_at_the_terminal_bottom() {
    let layout =
        TranscriptWorkspaceLayout::new(Rect::new(3, 5, 80, 24), /*composer_height*/ 5);

    assert_eq!(layout.transcript, Rect::new(3, 5, 80, 19));
    assert_eq!(layout.composer, Rect::new(3, 24, 80, 5));
}

#[test]
fn workspace_layout_clamps_a_tall_composer_to_the_available_area() {
    let layout = TranscriptWorkspaceLayout::new(Rect::new(0, 0, 40, 3), /*composer_height*/ 9);

    assert_eq!(layout.transcript, Rect::new(0, 0, 40, 0));
    assert_eq!(layout.composer, Rect::new(0, 0, 40, 3));
}

#[tokio::test]
async fn workspace_keeps_ctrl_c_available_to_the_existing_composer() {
    let mut overlay = TranscriptOverlay::new_workspace(
        Vec::new(),
        crate::keymap::RuntimeKeymap::defaults().pager,
    );
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");

    assert!(
        !overlay
            .handle_workspace_key(
                &mut tui,
                KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
            )
            .expect("workspace key routing")
    );
    assert!(!overlay.is_done());
}

#[test]
fn workspace_requests_older_history_only_for_upward_pager_navigation() {
    let mut overlay = TranscriptOverlay::new_workspace(
        Vec::new(),
        crate::keymap::RuntimeKeymap::defaults().pager,
    );
    overlay.view.scroll_offset = 0;
    overlay.view.last_content_height = Some(1);

    assert!(overlay.workspace_should_load_older(KeyEvent::from(KeyCode::PageUp)));
    assert!(!overlay.workspace_should_load_older(KeyEvent::from(KeyCode::PageDown)));
}
