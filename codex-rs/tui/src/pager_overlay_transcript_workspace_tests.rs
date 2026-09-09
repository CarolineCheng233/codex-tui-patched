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
                Rect::new(0, 0, 80, 24),
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
                Rect::new(0, 0, 80, 24),
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
                Rect::new(0, 0, 80, 24),
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
fn workspace_details_prepend_keeps_the_folded_turn() {
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
    assert!(overlay.workspace_turns.collapse_selected(&overlay.cells));
    assert!(overlay.open_workspace_details());

    overlay.prepend(
        vec![Arc::new(PlainHistoryCell::new(vec![Line::from(
            "older history",
        )]))],
        /*width*/ 80,
    );

    assert!(overlay.return_to_workspace());
    assert!(overlay.workspace_turns.is_collapsed(/*turn_start*/ 3));
    assert!(overlay.workspace_turns.is_cell_hidden(/*cell_index*/ 4));
}

#[test]
fn workspace_details_resize_restore_keeps_the_same_top_cell() {
    let cells = vec![
        Arc::new(PlainHistoryCell::new(vec![Line::from(
            "padding ".repeat(30),
        )])) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("saved anchor")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from(
            "newer content ".repeat(30),
        )])) as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let wide = Rect::new(0, 0, 80, 8);
    overlay.render_workspace(wide, &mut ratatui::buffer::Buffer::empty(wide));
    overlay.view.scroll_offset = overlay
        .workspace_layout_index
        .as_ref()
        .expect("wide workspace layout")
        .cells
        .iter()
        .find(|layout| layout.cell_index == 1)
        .expect("anchor cell in wide layout")
        .top;

    assert!(overlay.open_workspace_details());
    assert!(overlay.return_to_workspace());

    let narrow = Rect::new(0, 0, 20, 8);
    overlay.render_workspace(narrow, &mut ratatui::buffer::Buffer::empty(narrow));
    let anchor_top = overlay
        .workspace_layout_index
        .as_ref()
        .expect("narrow workspace layout")
        .cells
        .iter()
        .find(|layout| layout.cell_index == 1)
        .expect("anchor cell in narrow layout")
        .top;
    assert_eq!(overlay.view.scroll_offset, anchor_top);
}

#[test]
fn workspace_details_restore_deleted_anchor_to_previous_turn() {
    let previous_user = Arc::new(UserHistoryCell {
        message: "previous turn".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    }) as Arc<dyn crate::history_cell::HistoryCell>;
    let previous_reply = Arc::new(PlainHistoryCell::new(vec![Line::from("previous reply")]))
        as Arc<dyn crate::history_cell::HistoryCell>;
    let removed_user = Arc::new(UserHistoryCell {
        message: "removed turn".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    }) as Arc<dyn crate::history_cell::HistoryCell>;
    let removed_anchor = Arc::new(PlainHistoryCell::new(vec![Line::from("removed anchor")]))
        as Arc<dyn crate::history_cell::HistoryCell>;
    let newer_user = Arc::new(UserHistoryCell {
        message: "newer turn".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    }) as Arc<dyn crate::history_cell::HistoryCell>;
    let newer_reply = Arc::new(PlainHistoryCell::new(vec![Line::from(
        "newer reply ".repeat(30),
    )])) as Arc<dyn crate::history_cell::HistoryCell>;
    let mut overlay = TranscriptOverlay::new_workspace(
        vec![
            previous_user.clone(),
            previous_reply.clone(),
            removed_user,
            removed_anchor,
            newer_user.clone(),
            newer_reply.clone(),
        ],
        crate::keymap::RuntimeKeymap::defaults().pager,
    );
    let area = Rect::new(0, 0, 40, 8);
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    overlay.view.scroll_offset = overlay
        .workspace_layout_index
        .as_ref()
        .expect("workspace layout")
        .cells
        .iter()
        .find(|layout| layout.cell_index == 3)
        .expect("removed anchor layout")
        .top;
    assert!(overlay.open_workspace_details());
    overlay.replace_cells(vec![previous_user, previous_reply, newer_user, newer_reply]);

    assert!(overlay.return_to_workspace());
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    let previous_turn_top = overlay
        .workspace_layout_index
        .as_ref()
        .expect("restored workspace layout")
        .cells
        .iter()
        .find(|layout| layout.cell_index == 0)
        .expect("previous turn layout")
        .top;
    assert_eq!(overlay.view.scroll_offset, previous_turn_top);
}

#[test]
fn workspace_parity_details_append_anchor() {
    let cells = vec![
        Arc::new(PlainHistoryCell::new(vec![Line::from(
            "padding ".repeat(30),
        )])) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("saved anchor")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from(
            "existing tail ".repeat(30),
        )])) as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let area = Rect::new(0, 0, 40, 8);
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    overlay.view.scroll_offset = overlay
        .workspace_layout_index
        .as_ref()
        .expect("workspace layout")
        .cells
        .iter()
        .find(|layout| layout.cell_index == 1)
        .expect("anchor layout")
        .top;
    assert!(overlay.open_workspace_details());
    overlay.insert_cell(Arc::new(PlainHistoryCell::new(vec![Line::from(
        "appended once",
    )])));

    assert!(overlay.return_to_workspace());
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    let anchor_top = overlay
        .workspace_layout_index
        .as_ref()
        .expect("restored workspace layout")
        .cells
        .iter()
        .find(|layout| layout.cell_index == 1)
        .expect("anchor layout after append")
        .top;
    assert_eq!(overlay.view.scroll_offset, anchor_top);
    assert_eq!(overlay.cells.len(), 4);
}

#[test]
fn workspace_parity_details_follow_bottom() {
    let cells = vec![Arc::new(PlainHistoryCell::new(vec![Line::from(
        "existing content ".repeat(30),
    )])) as Arc<dyn crate::history_cell::HistoryCell>];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let area = Rect::new(0, 0, 40, 8);
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    overlay.view.scroll_offset = usize::MAX;
    assert!(overlay.open_workspace_details());
    overlay.insert_cell(Arc::new(PlainHistoryCell::new(vec![Line::from(
        "appended once",
    )])));

    assert!(overlay.return_to_workspace());
    overlay.render_workspace(area, &mut ratatui::buffer::Buffer::empty(area));
    assert!(overlay.view.is_scrolled_to_bottom());
    assert_eq!(overlay.cells.len(), 2);
}

#[test]
fn workspace_bottom_append_selects_the_new_user_turn() {
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(PlainHistoryCell::new(vec![Line::from("first reply")]))
            as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    overlay.view.scroll_offset = usize::MAX;

    overlay.insert_cell(Arc::new(UserHistoryCell {
        message: "second prompt".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    }));

    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(2));
}

#[tokio::test]
async fn workspace_bottom_append_preserves_a_manually_selected_turn() -> std::io::Result<()> {
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
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    overlay.view.scroll_offset = usize::MAX;

    overlay.handle_workspace_key(
        &mut tui,
        KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
        Rect::new(0, 0, 80, 10),
    )?;
    overlay.insert_cell(Arc::new(UserHistoryCell {
        message: "third prompt".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    }));

    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));
    Ok(())
}

#[tokio::test]
async fn workspace_scroll_restores_follow_viewport_before_a_bottom_append() -> std::io::Result<()> {
    let cells = vec![
        Arc::new(UserHistoryCell {
            message: "first prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
        Arc::new(UserHistoryCell {
            message: "second prompt".into(),
            text_elements: Vec::new(),
            local_image_paths: Vec::new(),
            remote_image_urls: Vec::new(),
        }) as Arc<dyn crate::history_cell::HistoryCell>,
    ];
    let mut overlay =
        TranscriptOverlay::new_workspace(cells, crate::keymap::RuntimeKeymap::defaults().pager);
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    overlay.view.scroll_offset = usize::MAX;

    overlay.handle_workspace_key(
        &mut tui,
        KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
        Rect::new(0, 0, 80, 10),
    )?;
    overlay.handle_workspace_mouse(
        &mut tui,
        MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        Rect::new(0, 0, 80, 10),
    )?;
    overlay.insert_cell(Arc::new(UserHistoryCell {
        message: "third prompt".into(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
    }));

    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(2));
    Ok(())
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

#[tokio::test]
async fn workspace_page_up_targets_the_turn_in_the_transcript_area() -> std::io::Result<()> {
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
    let layout =
        TranscriptWorkspaceLayout::new(Rect::new(0, 0, 80, 9), /* composer_height */ 3);
    overlay.view.scroll_offset = 1;

    overlay.handle_workspace_key(&mut tui, KeyEvent::from(KeyCode::PageUp), layout.transcript)?;

    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));
    Ok(())
}

#[tokio::test]
async fn workspace_wheel_targets_the_turn_in_the_transcript_area() -> std::io::Result<()> {
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
    let layout =
        TranscriptWorkspaceLayout::new(Rect::new(0, 0, 80, 9), /* composer_height */ 3);
    overlay.view.scroll_offset = 3;

    overlay.handle_workspace_mouse(
        &mut tui,
        MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        },
        layout.transcript,
    )?;

    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));
    Ok(())
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

    overlay.handle_workspace_key(
        &mut tui,
        KeyEvent::new(KeyCode::Up, KeyModifiers::ALT),
        Rect::new(0, 0, 72, 10),
    )?;
    assert_eq!(overlay.workspace_turns.selected_turn_start(), Some(0));

    overlay.handle_workspace_key(
        &mut tui,
        KeyEvent::new(KeyCode::Left, KeyModifiers::ALT),
        Rect::new(0, 0, 72, 10),
    )?;
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

    overlay.handle_workspace_key(
        &mut tui,
        KeyEvent::new(KeyCode::Right, KeyModifiers::ALT),
        Rect::new(0, 0, 72, 10),
    )?;
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
