//! Layout primitives for the interactive transcript workspace.

use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use ratatui::layout::Rect;
use std::collections::BTreeSet;
use std::sync::Arc;

pub(super) const LOCAL_IMAGE_PREVIEW_COLUMNS: u16 = 32;
pub(super) const LOCAL_IMAGE_PREVIEW_ROWS: u16 = 8;

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
            Self::Workspace => {
                "TRANSCRIPT · Option+Up/Down target · Option+Left fold · Option+Right expand · wheel/PgUp/PgDn scroll · Ctrl+T close"
            }
        }
    }
}

/// Records whether the current fold target follows the visible transcript or was chosen directly
/// with the turn-selection shortcuts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) enum WorkspaceTargetMode {
    #[default]
    FollowViewport,
    Manual,
}

/// Cached physical layout for one committed cell in the workspace transcript.
///
/// `top` is relative to the transcript content area (below the workspace header). Keeping the
/// base cell height and the optional image rows together lets wheel scrolling find the turn at the
/// viewport bottom without rebuilding renderables or measuring every cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceCellLayout {
    pub(super) cell_index: usize,
    pub(super) top: usize,
    pub(super) base_height: usize,
    pub(super) preview_rows: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceTurnLayout {
    pub(super) turn_start: usize,
    pub(super) top: usize,
    pub(super) bottom: usize,
}

/// Width-keyed index used by workspace scrolling and terminal image placement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct WorkspaceLayoutIndex {
    pub(super) width: u16,
    pub(super) cells: Vec<WorkspaceCellLayout>,
    pub(super) turns: Vec<WorkspaceTurnLayout>,
    pub(super) total_height: usize,
}

impl WorkspaceLayoutIndex {
    /// Select the last turn whose visible range begins before the viewport row.
    pub(super) fn turn_at_or_before(&self, row: usize) -> Option<usize> {
        let mut low = 0usize;
        let mut high = self.turns.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if self.turns[mid].top <= row {
                low = mid.saturating_add(1);
            } else {
                high = mid;
            }
        }
        low.checked_sub(1)
            .and_then(|index| self.turns.get(index))
            .map(|turn| turn.turn_start)
    }
}

/// Tracks whole user turns for the transcript workspace without copying cells.
///
/// A turn starts at a user cell and includes all following cells up to the next user cell.
/// Collapsing a turn keeps its prompt visible and hides only its subsequent cells.
#[derive(Debug, Default)]
pub(super) struct TranscriptTurnState {
    turn_starts: Vec<usize>,
    selected_turn: Option<usize>,
    collapsed_turn_starts: BTreeSet<usize>,
}

impl TranscriptTurnState {
    pub(super) fn new(cells: &[Arc<dyn HistoryCell>]) -> Self {
        let mut state = Self::default();
        state.reset(cells);
        state
    }

    pub(super) fn selected_turn_start(&self) -> Option<usize> {
        self.selected_turn
    }

    pub(super) fn select_turn_start(&mut self, turn_start: usize) -> bool {
        if self.selected_turn == Some(turn_start)
            || self.turn_starts.binary_search(&turn_start).is_err()
        {
            return false;
        }
        self.selected_turn = Some(turn_start);
        true
    }

    pub(super) fn select_previous(&mut self) -> bool {
        let Some(selected) = self.selected_turn else {
            return false;
        };
        let Some(position) = self.turn_starts.iter().position(|&start| start == selected) else {
            return false;
        };
        let Some(previous) = position
            .checked_sub(1)
            .and_then(|index| self.turn_starts.get(index))
        else {
            return false;
        };
        self.selected_turn = Some(*previous);
        true
    }

    pub(super) fn select_next(&mut self) -> bool {
        let Some(selected) = self.selected_turn else {
            return false;
        };
        let Some(position) = self.turn_starts.iter().position(|&start| start == selected) else {
            return false;
        };
        let Some(next) = self.turn_starts.get(position.saturating_add(1)) else {
            return false;
        };
        self.selected_turn = Some(*next);
        true
    }

    pub(super) fn select_latest_turn(&mut self) -> bool {
        let Some(latest) = self.turn_starts.last().copied() else {
            return false;
        };
        self.select_turn_start(latest)
    }

    pub(super) fn collapse_selected(&mut self, cells: &[Arc<dyn HistoryCell>]) -> bool {
        let Some(selected) = self.selected_turn else {
            return false;
        };
        if self.hidden_cell_count_after(selected, cells) == 0 {
            return false;
        }
        self.collapsed_turn_starts.insert(selected)
    }

    pub(super) fn expand_selected(&mut self) -> bool {
        self.selected_turn
            .is_some_and(|selected| self.collapsed_turn_starts.remove(&selected))
    }

    pub(super) fn is_cell_hidden(&self, cell_index: usize) -> bool {
        let Some(turn_start) = self.collapsed_turn_starts.range(..=cell_index).next_back() else {
            return false;
        };
        *turn_start < cell_index
            && self
                .turn_starts
                .iter()
                .copied()
                .find(|&start| start > *turn_start)
                .is_none_or(|next_turn_start| cell_index < next_turn_start)
    }

    pub(super) fn hidden_cell_count_after(
        &self,
        turn_start: usize,
        cells: &[Arc<dyn HistoryCell>],
    ) -> usize {
        self.turn_starts
            .iter()
            .copied()
            .find(|&next_turn_start| next_turn_start > turn_start)
            .unwrap_or(cells.len())
            .saturating_sub(turn_start.saturating_add(1))
    }

    pub(super) fn is_collapsed(&self, turn_start: usize) -> bool {
        self.collapsed_turn_starts.contains(&turn_start)
    }

    pub(super) fn reset(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        self.turn_starts = turn_starts(cells);
        self.collapsed_turn_starts.clear();
        self.selected_turn = self.turn_starts.last().copied();
    }

    pub(super) fn refresh_after_append(&mut self, cells: &[Arc<dyn HistoryCell>]) {
        self.turn_starts = turn_starts(cells);
        self.collapsed_turn_starts
            .retain(|start| self.turn_starts.binary_search(start).is_ok());
        if self
            .selected_turn
            .is_none_or(|selected| self.turn_starts.binary_search(&selected).is_err())
        {
            self.selected_turn = self.turn_starts.last().copied();
        }
    }

    pub(super) fn shift_indices_from(&mut self, insert_at: usize, added_cells: usize) {
        let shift = |index: &mut usize| {
            if *index >= insert_at {
                *index = index.saturating_add(added_cells);
            }
        };
        if let Some(selected) = self.selected_turn.as_mut() {
            shift(selected);
        }
        self.collapsed_turn_starts = self
            .collapsed_turn_starts
            .iter()
            .map(|start| {
                let mut start = *start;
                shift(&mut start);
                start
            })
            .collect();
    }
}

fn turn_starts(cells: &[Arc<dyn HistoryCell>]) -> Vec<usize> {
    cells
        .iter()
        .enumerate()
        .filter_map(|(index, cell)| cell.as_any().is::<UserHistoryCell>().then_some(index))
        .collect()
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
