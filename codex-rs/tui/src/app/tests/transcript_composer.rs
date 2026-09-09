//! Regression coverage for transcript viewer input and prompt selection.
//!
//! The default-off feature must leave the existing viewer and its draft intact.

use super::*;
use crate::bottom_pane::SelectionItem;
use crate::bottom_pane::SelectionViewParams;
use crate::history_cell::PlainHistoryCell;
use crate::pager_overlay::TranscriptWorkspaceLayout;
use crate::render::renderable::Renderable;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyModifiers;
use pretty_assertions::assert_eq;
use ratatui::widgets::Clear;
use ratatui::widgets::Widget;

async fn press_key(
    app: &mut App,
    tui: &mut crate::tui::Tui,
    app_server: &mut AppServerSession,
    code: KeyCode,
) -> Result<()> {
    app.handle_tui_event(
        tui,
        app_server,
        TuiEvent::Key(KeyEvent::new(code, KeyModifiers::NONE)),
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn transcript_flag_off_preserves_viewer_and_backtracking() -> Result<()> {
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    let keymap_config = toml::from_str("[composer]\nsubmit = [\"ctrl-x enter\"]")?;
    app.keymap =
        crate::keymap::RuntimeKeymap::from_config(&keymap_config).expect("valid composer chord");
    app.chat_widget
        .apply_keymap_update(keymap_config, &app.keymap);
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.transcript_cells = ["first", "second"]
        .map(|message| {
            Arc::new(UserHistoryCell {
                message: message.into(),
                text_elements: Vec::new(),
                local_image_paths: Vec::new(),
                remote_image_urls: Vec::new(),
            }) as Arc<dyn HistoryCell>
        })
        .to_vec();
    app.chat_widget
        .apply_external_edit("preserved draft".into());
    app.open_transcript_overlay(&mut tui);
    for event in [
        TuiEvent::Paste("not composer input".into()),
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)),
    ] {
        app.handle_tui_event(&mut tui, &mut app_server, event)
            .await?;
    }
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "preserved draft"
    );
    let chord_prefix = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Key(chord_prefix))
        .await?;
    assert!(!app.key_chord_matcher.is_pending());
    assert!(!app.backtrack.overlay_preview_active);
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 100, /*height*/ 12,
    );
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    let Some(Overlay::Transcript(overlay)) = &mut app.overlay else {
        panic!("viewer closed")
    };
    overlay.render(area, &mut buffer);
    let text = buffer
        .content()
        .chunks(usize::from(area.width))
        .map(|row| {
            row.iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!("transcript_flag_off_viewer", text);
    for (key, selected) in [
        (KeyCode::Esc, 1),
        (KeyCode::Esc, 0),
        (KeyCode::Right, 1),
        (KeyCode::Right, 1),
    ] {
        press_key(&mut app, &mut tui, &mut app_server, key).await?;
        assert_eq!(app.backtrack.nth_user_message, selected);
    }
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Enter).await?;
    assert!(app.overlay.is_none());
    assert!(
        std::iter::from_fn(|| app_event_rx.try_recv().ok()).any(|event| matches!(
            event,
            AppEvent::ForkSessionForPromptEdit {
                nth_user_message: 1,
                ..
            }
        ))
    );
    Ok(())
}

#[tokio::test]
async fn transcript_workspace_routes_typing_and_ctrl_c_to_the_existing_composer() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.transcript_cells = ["first", "second"]
        .map(|message| {
            Arc::new(UserHistoryCell {
                message: message.into(),
                text_elements: Vec::new(),
                local_image_paths: Vec::new(),
                remote_image_urls: Vec::new(),
            }) as Arc<dyn HistoryCell>
        })
        .to_vec();

    app.open_transcript_overlay(&mut tui);
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if overlay.is_workspace()
    ));

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Paste("workspace draft".into()),
    )
    .await?;
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "workspace draft"
    );

    let text = {
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 72, /*height*/ 12,
        );
        let composer = app.chat_widget.transcript_workspace_bottom_pane();
        let layout = TranscriptWorkspaceLayout::new(area, composer.desired_height(area.width));
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        let Some(Overlay::Transcript(overlay)) = &mut app.overlay else {
            panic!("workspace closed")
        };
        overlay.render_workspace(layout.transcript, &mut buffer);
        Clear.render(layout.composer, &mut buffer);
        composer.render(layout.composer, &mut buffer);
        buffer
            .content()
            .chunks(usize::from(area.width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    insta::assert_snapshot!("transcript_workspace_composer_bottom", text);

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::PageUp, KeyModifiers::NONE)),
    )
    .await?;
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "workspace draft"
    );

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(app.chat_widget.composer_is_empty());
    assert!(app.overlay.is_some());

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Paste("cleared by ctrl-u".into()),
    )
    .await?;
    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(app.chat_widget.composer_is_empty());
    assert!(app.transcript_workspace_active());

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if !overlay.is_workspace()
    ));
    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(app.transcript_workspace_active());
    app.close_transcript_overlay(&mut tui);
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_input_space_reaches_composer() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget.apply_external_edit("hello".into());

    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char(' ')).await?;

    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;

    assert_eq!(app.chat_widget.composer_text_with_pending(), "hello ");
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_input_shift_space_reaches_composer() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget.apply_external_edit("hello".into());

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::SHIFT)),
    )
    .await?;
    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;

    assert_eq!(app.chat_widget.composer_text_with_pending(), "hello ");
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_input_home_end_reaches_composer() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget.apply_external_edit("abc".into());

    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Home).await?;
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char('X')).await?;
    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(app.chat_widget.composer_text_with_pending(), "Xabc");

    press_key(&mut app, &mut tui, &mut app_server, KeyCode::End).await?;
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char('Y')).await?;
    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(app.chat_widget.composer_text_with_pending(), "XabcY");
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_input_ctrl_b_f_reaches_composer() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget.apply_external_edit("abc".into());

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('b'), KeyModifiers::CONTROL)),
    )
    .await?;
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char('X')).await?;
    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(app.chat_widget.composer_text_with_pending(), "abXc");

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)),
    )
    .await?;
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char('Y')).await?;
    std::thread::sleep(crate::bottom_pane::ChatComposer::recommended_paste_flush_delay());
    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;
    assert_eq!(app.chat_widget.composer_text_with_pending(), "abXcY");
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_parity_details_roundtrip_preserves_the_draft() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget
        .apply_external_edit("preserved draft".into());

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if !overlay.is_workspace()
    ));

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if overlay.is_workspace()
    ));
    assert_eq!(
        app.chat_widget.composer_text_with_pending(),
        "preserved draft"
    );
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_parity_details_close_returns_to_workspace() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
    )
    .await?;

    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if overlay.is_workspace()
    ));
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_parity_details_thread_change_clears_old_overlay() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = crate::start_embedded_app_server_for_picker(&app.config).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.transcript_cells
        .push(Arc::new(PlainHistoryCell::new(vec![
            "old thread history".into(),
        ])));
    app.open_transcript_overlay(&mut tui);
    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if !overlay.is_workspace()
    ));

    let started = app_server.start_thread(&app.config).await?;
    app.replace_chat_widget_with_app_server_thread(
        &mut tui,
        started,
        crate::app::session_lifecycle::ThreadAttachPresentation::SessionLineage,
        /*initial_user_message*/ None,
    )
    .await?;

    assert!(app.overlay.is_none());
    assert!(
        app.transcript_cells.is_empty(),
        "new thread must not keep old thread history"
    );
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_parity_details_terminal_lifecycle() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);

    app.open_transcript_overlay(&mut tui);
    assert!(tui.is_alt_screen_active());
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char('t')).await?;
    assert!(tui.is_alt_screen_active());
    press_key(&mut app, &mut tui, &mut app_server, KeyCode::Char('t')).await?;
    assert!(tui.is_alt_screen_active());

    app.close_transcript_overlay(&mut tui);
    assert!(app.overlay.is_none());
    assert!(!tui.is_alt_screen_active());
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_parity_details_prepend_sync() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.transcript_cells
        .push(Arc::new(PlainHistoryCell::new(vec![
            "current history".into(),
        ])));
    app.open_transcript_overlay(&mut tui);

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    let Some(Overlay::Transcript(overlay)) = app.overlay.as_mut() else {
        panic!("viewer closed")
    };
    overlay.prepend(
        vec![Arc::new(PlainHistoryCell::new(vec![
            "older history".into(),
        ]))],
        /*width*/ 80,
    );

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
    )
    .await?;
    let area = Rect::new(
        /*x*/ 0, /*y*/ 0, /*width*/ 80, /*height*/ 10,
    );
    let mut buffer = ratatui::buffer::Buffer::empty(area);
    let Some(Overlay::Transcript(overlay)) = app.overlay.as_mut() else {
        panic!("workspace closed")
    };
    overlay.render_workspace(area, &mut buffer);
    let rendered = buffer
        .content()
        .iter()
        .map(ratatui::buffer::Cell::symbol)
        .collect::<String>();
    assert!(rendered.contains("older history"));
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn workspace_input_popup_owns_page_navigation() -> Result<()> {
    let (mut app, mut app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let session = test_thread_session(ThreadId::new(), app.config.cwd.to_path_buf());
    app.chat_widget.handle_thread_session(session);
    app.open_transcript_overlay(&mut tui);
    app.chat_widget.show_selection_view(SelectionViewParams {
        items: vec![
            SelectionItem {
                name: "First".to_string(),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenSkillsList))],
                dismiss_on_select: true,
                ..Default::default()
            },
            SelectionItem {
                name: "Second".to_string(),
                actions: vec![Box::new(|tx| tx.send(AppEvent::OpenManageSkillsPopup))],
                dismiss_on_select: true,
                ..Default::default()
            },
        ],
        ..Default::default()
    });

    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::PageDown, KeyModifiers::NONE)),
    )
    .await?;
    app.handle_tui_event(
        &mut tui,
        &mut app_server,
        TuiEvent::Key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
    )
    .await?;

    let events = std::iter::from_fn(|| app_event_rx.try_recv().ok()).collect::<Vec<_>>();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, AppEvent::OpenManageSkillsPopup)),
        "expected the second popup action, got {events:?}"
    );
    app_server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn enabled_transcript_workspace_opens_on_the_first_draw() -> Result<()> {
    let (mut app, _app_event_rx, _op_rx) = make_test_app_with_channels().await;
    app.local_settings.tui.transcript_workspace = true;
    let mut app_server = start_config_write_test_app_server(&app).await?;
    let mut tui = crate::tui::test_support::make_test_tui()?;

    app.handle_tui_event(&mut tui, &mut app_server, TuiEvent::Draw)
        .await?;

    assert!(matches!(
        &app.overlay,
        Some(Overlay::Transcript(overlay)) if overlay.is_workspace()
    ));
    app_server.shutdown().await?;
    Ok(())
}
