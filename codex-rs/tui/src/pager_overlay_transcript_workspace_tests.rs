use super::TranscriptOverlay;
use super::transcript_workspace::TranscriptTurnState;
use super::transcript_workspace::TranscriptWorkspaceLayout;
use crate::history_cell::PlainHistoryCell;
use crate::history_cell::UserHistoryCell;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::layout::Rect;
use ratatui::text::Line;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

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

#[tokio::test]
async fn workspace_wheel_scrolls_only_the_transcript() {
    let mut overlay = TranscriptOverlay::new_workspace(
        Vec::new(),
        crate::keymap::RuntimeKeymap::defaults().pager,
    );
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    overlay.view.scroll_offset = 6;

    assert!(
        overlay
            .handle_workspace_mouse(
                &mut tui,
                MouseEvent {
                    kind: MouseEventKind::ScrollUp,
                    column: 0,
                    row: 0,
                    modifiers: KeyModifiers::NONE,
                },
            )
            .expect("workspace mouse routing")
    );
    assert_eq!(overlay.view.scroll_offset, 3);

    assert!(
        overlay
            .handle_workspace_mouse(
                &mut tui,
                MouseEvent {
                    kind: MouseEventKind::ScrollDown,
                    column: 0,
                    row: 0,
                    modifiers: KeyModifiers::NONE,
                },
            )
            .expect("workspace mouse routing")
    );
    assert_eq!(overlay.view.scroll_offset, 6);
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

#[test]
fn turn_state_folds_one_user_turn_and_preserves_its_selection() {
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("first reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(UserHistoryCell {
            message: "second prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("second reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut turns = TranscriptTurnState::new(&cells);

    assert_eq!(turns.selected_turn_start(), Some(2));
    assert!(turns.select_previous());
    assert_eq!(turns.selected_turn_start(), Some(0));
    assert!(turns.collapse_selected(&cells));
    assert!(turns.is_cell_hidden(1));
    assert!(!turns.is_cell_hidden(2));
    assert_eq!(turns.hidden_cell_count_after(0, &cells), 1);
    assert_eq!(turns.selected_turn_start(), Some(0));
    assert!(turns.select_next());
    assert_eq!(turns.selected_turn_start(), Some(2));
}

#[test]
fn workspace_scroll_targets_the_turn_above_the_bottom_bar() {
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("first reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(UserHistoryCell {
            message: "second prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("second reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);

    // The overlay starts with the latest turn selected. At the top of this
    // short viewport, the first turn is the one immediately above its bottom
    // bar and must become the fold target.
    overlay.view.scroll_offset = 0;
    overlay.sync_workspace_turn_to_viewport(Rect::new(0, 0, 80, 6));

    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));
    assert!(overlay.workspace_turns.collapse_selected(&overlay.cells));
    assert!(overlay.workspace_turns.is_collapsed(0));
    assert!(!overlay.workspace_turns.is_collapsed(2));
}

#[test]
fn workspace_scroll_reuses_the_width_keyed_layout_index() {
    #[derive(Debug)]
    struct CountingCell {
        measurements: AtomicUsize,
    }

    impl crate::history_cell::HistoryCell for CountingCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            vec![Line::from("tool output")]
        }

        fn raw_lines(&self) -> Vec<Line<'static>> {
            vec![Line::from("tool output")]
        }

        fn desired_workspace_transcript_height(&self, _width: u16) -> u16 {
            self.measurements.fetch_add(1, Ordering::Relaxed);
            1
        }
    }

    let measured = Arc::new(CountingCell {
        measurements: AtomicUsize::new(0),
    });
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        measured.clone() as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(UserHistoryCell {
            message: "second prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("second reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let area = Rect::new(0, 0, 80, 4);
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    let measurements_after_first_render = measured.measurements.load(Ordering::Relaxed);

    for _ in 0..32 {
        overlay.sync_workspace_turn_to_viewport(area);
    }

    overlay.view.scroll_offset = 0;
    overlay.sync_workspace_turn_to_viewport(area);
    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));

    assert_eq!(
        measured.measurements.load(Ordering::Relaxed),
        measurements_after_first_render,
        "repeated wheel-target synchronization must reuse the cached layout",
    );
}

#[tokio::test]
async fn workspace_turn_keys_select_collapse_and_expand_a_whole_turn() -> std::io::Result<()> {
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("first reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(UserHistoryCell {
            message: "second prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("second reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");

    overlay.handle_workspace_key(&mut tui, KeyEvent::new(KeyCode::Up, KeyModifiers::ALT))?;
    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));

    overlay.handle_workspace_key(&mut tui, KeyEvent::new(KeyCode::Left, KeyModifiers::ALT))?;
    assert!(overlay.workspace_turns.is_collapsed(0));
    assert!(overlay.workspace_turns.is_cell_hidden(1));

    let area = Rect::new(0, 0, 72, 10);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    overlay.render_workspace(area, &mut buffer);
    let rendered = buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("1 hidden transcript item(s)"));
    assert!(!rendered.contains("first reply"));

    overlay.handle_workspace_key(&mut tui, KeyEvent::new(KeyCode::Right, KeyModifiers::ALT))?;
    assert!(!overlay.workspace_turns.is_collapsed(0));
    Ok(())
}

#[test]
fn workspace_marks_the_selected_user_turn_without_changing_other_markers() {
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("first reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(UserHistoryCell {
            message: "second prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let area = Rect::new(0, 0, 80, 20);
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    overlay.render_workspace(area, &mut buffer);
    let rendered = buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();

    assert!(rendered.contains("› first prompt"));
    assert!(rendered.contains("▸ second prompt"));

    let small_area = Rect::new(0, 0, 80, 5);
    overlay.view.scroll_offset = 0;
    overlay.sync_workspace_turn_to_viewport(small_area);
    let mut buffer = ratatui::buffer::Buffer::empty(small_area);
    overlay.render_workspace(small_area, &mut buffer);
    let rendered = buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("▸ first prompt"));
}

#[test]
fn workspace_places_local_input_images_in_reserved_user_rows() {
    let dir = tempfile::tempdir().expect("temp directory");
    let path = dir.path().join("input-image.png");
    std::fs::write(&path, b"png").expect("test image");
    let cells = vec![Arc::new(UserHistoryCell {
        message: "[Image #1]".into(),
        text_elements: Vec::new(),
        local_image_paths: vec![path.clone()],
        remote_image_urls: Vec::new(),
    }) as Arc<dyn crate::history_cell::HistoryCell>];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    overlay.set_local_image_previews_enabled(true);
    overlay.view.scroll_offset = 0;

    let previews = overlay.workspace_local_image_previews(Rect::new(0, 0, 80, 30));

    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0].path, path);
    assert_eq!(previews[0].columns, 32);
    assert_eq!(previews[0].rows, 8);
    assert!(previews[0].y > 0);
}
