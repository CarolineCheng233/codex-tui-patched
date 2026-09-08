//! Input routing for the interactive transcript workspace.

use super::*;

/// Computes workspace geometry from the composer state at the instant it is needed.
///
/// Input handling cannot reuse the previous frame's rectangle: a draft can change the composer
/// height before the next frame is rendered.
pub(super) fn transcript_workspace_layout(
    chat_widget: &crate::chatwidget::ChatWidget,
    area: ratatui::layout::Rect,
) -> crate::pager_overlay::TranscriptWorkspaceLayout {
    let composer = chat_widget.transcript_workspace_bottom_pane();
    crate::pager_overlay::TranscriptWorkspaceLayout::new(
        area,
        composer.desired_height(area.width.max(1)),
    )
}

impl App {
    pub(super) async fn handle_transcript_workspace_event(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        event: TuiEvent,
    ) -> Result<bool> {
        let should_load_older = match (&event, self.overlay.as_ref()) {
            (TuiEvent::Key(key_event), Some(Overlay::Transcript(overlay))) => {
                overlay.workspace_should_load_older(*key_event)
            }
            (TuiEvent::Mouse(mouse_event), Some(Overlay::Transcript(overlay))) => {
                overlay.workspace_wheel_should_load_older(*mouse_event)
            }
            _ => false,
        };
        let load_from_start = matches!(
            (&event, self.overlay.as_ref()),
            (TuiEvent::Key(key_event), Some(Overlay::Transcript(overlay)))
                if overlay.should_load_from_start(*key_event)
        );
        if should_load_older
            && let Some(thread_id) = self.chat_widget.thread_id()
            && app_server.has_older_history(thread_id)
            && self.request_older_history_page(app_server, thread_id)
        {
            if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                overlay.set_history_state(if load_from_start {
                    TranscriptHistoryState::LoadingBeginning
                } else {
                    TranscriptHistoryState::LoadingOlder
                });
            }
            tui.frame_requester().schedule_frame();
        }
        match event {
            TuiEvent::Key(key_event) => {
                let transcript_area =
                    transcript_workspace_layout(&self.chat_widget, tui.terminal.viewport_area)
                        .transcript;
                let handled = if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                    overlay.handle_workspace_key(tui, key_event, transcript_area)?
                } else {
                    false
                };
                if handled {
                    if self.overlay.as_ref().is_some_and(Overlay::is_done) {
                        self.close_transcript_overlay(tui);
                    }
                    return Ok(true);
                }
                self.chat_widget.handle_key_event(key_event);
            }
            TuiEvent::Paste(pasted) => {
                let pasted = pasted.replace("\r\n", "\n").replace('\r', "\n");
                self.chat_widget.handle_paste(pasted);
            }
            TuiEvent::Mouse(mouse_event) => {
                let transcript_area =
                    transcript_workspace_layout(&self.chat_widget, tui.terminal.viewport_area)
                        .transcript;
                if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                    overlay.handle_workspace_mouse(tui, mouse_event, transcript_area)?;
                }
            }
            event @ (TuiEvent::Draw
            | TuiEvent::Resume
            | TuiEvent::Resize(_)
            | TuiEvent::FocusGained) => {
                self.chat_widget.maybe_post_pending_notification(tui);
                if self
                    .chat_widget
                    .handle_paste_burst_tick(tui.frame_requester())
                {
                    return Ok(true);
                }
                self.chat_widget.pre_draw_tick();
                self.overlay_forward_event(tui, event)?;
            }
            event => self.overlay_forward_event(tui, event)?,
        }
        Ok(true)
    }
}
