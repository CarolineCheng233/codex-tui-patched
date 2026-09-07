//! Input routing for the interactive transcript workspace.

use super::*;

impl App {
    pub(super) async fn handle_transcript_workspace_event(
        &mut self,
        tui: &mut tui::Tui,
        app_server: &mut AppServerSession,
        event: TuiEvent,
    ) -> Result<bool> {
        if let TuiEvent::Key(key_event) = &event
            && let Some(Overlay::Transcript(overlay)) = self.overlay.as_ref()
            && overlay.workspace_should_load_older(*key_event)
            && let Some(thread_id) = self.chat_widget.thread_id()
            && app_server.has_older_history(thread_id)
            && self.request_older_history_page(app_server, thread_id)
        {
            if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                overlay.set_history_state(if overlay.should_load_from_start(*key_event) {
                    TranscriptHistoryState::LoadingBeginning
                } else {
                    TranscriptHistoryState::LoadingOlder
                });
            }
            tui.frame_requester().schedule_frame();
        }
        match event {
            TuiEvent::Key(key_event) => {
                let handled = if let Some(Overlay::Transcript(overlay)) = self.overlay.as_mut() {
                    overlay.handle_workspace_key(tui, key_event)?
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
