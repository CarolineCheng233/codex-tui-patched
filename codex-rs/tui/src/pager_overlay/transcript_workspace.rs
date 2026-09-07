//! Layout primitives for the interactive transcript workspace.

use ratatui::layout::Rect;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum TranscriptMode {
    Viewer,
    Workspace,
}

impl TranscriptMode {
    pub(super) fn is_workspace(self) -> bool {
        matches!(self, Self::Workspace)
    }

    pub(super) fn title(self) -> &'static str {
        match self {
            Self::Viewer => "T R A N S C R I P T",
            Self::Workspace => "T R A N S C R I P T  ·  PgUp/PgDn scroll  ·  Ctrl+T close",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TranscriptWorkspaceLayout {
    pub(crate) transcript: Rect,
    pub(crate) composer: Rect,
}

impl TranscriptWorkspaceLayout {
    pub(crate) fn new(area: Rect, composer_height: u16) -> Self {
        let composer_height = composer_height.min(area.height);
        let transcript_height = area.height.saturating_sub(composer_height);
        Self {
            transcript: Rect::new(area.x, area.y, area.width, transcript_height),
            composer: Rect::new(
                area.x,
                area.y.saturating_add(transcript_height),
                area.width,
                composer_height,
            ),
        }
    }
}
