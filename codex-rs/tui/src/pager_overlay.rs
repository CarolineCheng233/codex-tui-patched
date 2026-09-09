//! Overlay UIs rendered in an alternate screen.
//!
//! This module implements the pager-style overlays used by the TUI, including the transcript
//! overlay (`Ctrl+T`) that renders a full history view separate from the main viewport.
//!
//! The transcript overlay renders committed transcript cells plus an optional render-only live tail
//! derived from the current in-flight active cell. Because rebuilding wrapped `Line`s on every draw
//! can be expensive, that live tail is cached and only recomputed when its cache key changes, which
//! is derived from the terminal width (wrapping), an active-cell revision (in-place mutations), the
//! stream-continuation flag (spacing), and an animation tick (time-based spinner/shimmer output).
//!
//! The transcript overlay live tail is kept in sync by `App` during draws: `App` supplies an
//! `ActiveCellTranscriptKey` and a function to compute the active cell transcript lines, and
//! `TranscriptOverlay::sync_live_tail` uses the key to decide when the cached tail must be
//! recomputed. `ChatWidget` is responsible for producing a key that changes when the active cell
//! mutates in place or when its transcript output is time-dependent.

mod scrolling;
mod transcript_workspace;

#[cfg(test)]
#[path = "pager_overlay_transcript_workspace_tests.rs"]
mod transcript_workspace_tests;

#[cfg(test)]
#[path = "pager_overlay/highlight_tests.rs"]
mod highlight_tests;

use std::cell::Cell as TargetCell;
use std::io::Result;
use std::rc::Rc;
use std::sync::Arc;

use crate::chatwidget::ActiveCellTranscriptKey;
use crate::history_cell::HistoryCell;
use crate::history_cell::SessionInfoCell;
use crate::history_cell::UserHistoryCell;
use crate::key_hint;
use crate::key_hint::KeyBinding;
use crate::key_hint::KeyBindingListExt;
use crate::key_hint::ShortcutHint;
use crate::keymap::PagerKeymap;
use crate::live_wrap::take_prefix_by_width;
use crate::render::Insets;
use crate::render::renderable::InsetRenderable;
use crate::render::renderable::Renderable;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::tui;
use crate::tui::TuiEvent;
use crate::width::display_width;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::MouseEvent;
use crossterm::event::MouseEventKind;
use ratatui::buffer::Buffer;
use ratatui::buffer::Cell;
use ratatui::layout::Rect;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Clear;
use ratatui::widgets::Paragraph;
use ratatui::widgets::Widget;
use ratatui::widgets::Wrap;
use scrolling::CellRenderable;
use scrolling::HyperlinkLinesRenderable;
use scrolling::render_offset_content;
use transcript_workspace::LOCAL_IMAGE_PREVIEW_COLUMNS;
use transcript_workspace::LOCAL_IMAGE_PREVIEW_ROWS;
use transcript_workspace::TranscriptMode;
use transcript_workspace::TranscriptTurnState;
pub(crate) use transcript_workspace::TranscriptWorkspaceLayout;
use transcript_workspace::WorkspaceCellLayout;
use transcript_workspace::WorkspaceLayoutIndex;
use transcript_workspace::WorkspaceTargetMode;
use transcript_workspace::WorkspaceTurnLayout;

pub(crate) enum Overlay {
    Transcript(TranscriptOverlay),
    Static(StaticOverlay),
}

impl Overlay {
    pub(crate) fn new_transcript(cells: Vec<Arc<dyn HistoryCell>>, keymap: PagerKeymap) -> Self {
        Self::Transcript(TranscriptOverlay::new(cells, keymap))
    }

    pub(crate) fn new_transcript_workspace(
        cells: Vec<Arc<dyn HistoryCell>>,
        keymap: PagerKeymap,
    ) -> Self {
        Self::Transcript(TranscriptOverlay::new_workspace(cells, keymap))
    }

    pub(crate) fn new_static_with_lines(
        lines: Vec<Line<'static>>,
        title: String,
        keymap: PagerKeymap,
    ) -> Self {
        Self::Static(StaticOverlay::with_title(lines, title, keymap))
    }

    pub(crate) fn new_static_with_renderables(
        renderables: Vec<Box<dyn Renderable>>,
        title: String,
        keymap: PagerKeymap,
    ) -> Self {
        Self::Static(StaticOverlay::with_renderables(renderables, title, keymap))
    }

    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match self {
            Overlay::Transcript(o) => o.handle_event(tui, event),
            Overlay::Static(o) => o.handle_event(tui, event),
        }
    }

    pub(crate) fn is_done(&self) -> bool {
        match self {
            Overlay::Transcript(o) => o.is_done(),
            Overlay::Static(o) => o.is_done(),
        }
    }
}

fn first_or_empty(
    keymap: &PagerKeymap,
    action: &'static str,
    bindings: &[KeyBinding],
) -> Vec<ShortcutHint> {
    keymap.primary_hint(action, bindings).into_iter().collect()
}

// Render a single line of key hints from (key(s), description) pairs.
fn render_key_hints(area: Rect, buf: &mut Buffer, pairs: &[(Vec<ShortcutHint>, &str)]) {
    let mut spans: Vec<Span<'static>> = vec![" ".into()];
    let mut first = true;
    for (keys, desc) in pairs {
        if !first {
            spans.push("   ".into());
        }
        for (i, key) in keys.iter().enumerate() {
            if i > 0 {
                spans.push("/".into());
            }
            spans.push(Span::from(*key));
        }
        spans.push(" ".into());
        spans.push(Span::from(desc.to_string()));
        first = false;
    }
    Paragraph::new(vec![Line::from(spans).dim()]).render(area, buf);
}

fn render_navigation_hints(area: Rect, buf: &mut Buffer, keymap: &PagerKeymap) {
    let actions = [
        ("scroll_up", &keymap.scroll_up),
        ("scroll_down", &keymap.scroll_down),
        ("page_up", &keymap.page_up),
        ("page_down", &keymap.page_down),
        ("jump_top", &keymap.jump_top),
        ("jump_bottom", &keymap.jump_bottom),
    ];
    let hints = actions
        .chunks_exact(2)
        .zip(["to scroll", "to page", "to jump"])
        .map(|(actions, description)| {
            (
                actions
                    .iter()
                    .filter_map(|(action, bindings)| keymap.primary_hint(action, bindings))
                    .collect(),
                description,
            )
        })
        .collect::<Vec<_>>();
    render_key_hints(area, buf, &hints);
}

/// Generic widget for rendering a pager view.
struct PagerView {
    renderables: Vec<Box<dyn Renderable>>,
    scroll_offset: usize,
    title: String,
    keymap: PagerKeymap,
    last_content_height: Option<usize>,
    last_rendered_height: Option<usize>,
    /// Percentages are meaningful only when the full scrollable history is known.
    scroll_percentage_visible: bool,
    /// If set, on next render ensure this chunk is visible.
    pending_scroll_chunk: Option<usize>,
    /// Workspace-only header treatment. Generic pager overlays retain their legacy header.
    workspace_header: bool,
}

impl PagerView {
    fn new(
        renderables: Vec<Box<dyn Renderable>>,
        title: String,
        scroll_offset: usize,
        keymap: PagerKeymap,
    ) -> Self {
        Self {
            renderables,
            scroll_offset,
            title,
            keymap,
            last_content_height: None,
            last_rendered_height: None,
            scroll_percentage_visible: true,
            pending_scroll_chunk: None,
            workspace_header: false,
        }
    }

    fn content_height(&self, width: u16) -> usize {
        self.renderables
            .iter()
            .map(|c| c.desired_height(width) as usize)
            .sum()
    }

    fn render(&mut self, area: Rect, buf: &mut Buffer) {
        Clear.render(area, buf);
        self.render_header(area, buf);
        let content_area = self.content_area(area);
        self.update_last_content_height(content_area.height);
        let content_height = self.content_height(content_area.width);
        self.last_rendered_height = Some(content_height);
        // If there is a pending request to scroll a specific chunk into view,
        // satisfy it now that wrapping is up to date for this width.
        if let Some(idx) = self.pending_scroll_chunk.take() {
            self.ensure_chunk_visible(idx, content_area);
        }
        self.scroll_offset = self
            .scroll_offset
            .min(content_height.saturating_sub(content_area.height as usize));

        self.render_content(content_area, buf);

        self.render_bottom_bar(area, content_area, buf, content_height);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        if self.workspace_header {
            Span::from("─".repeat(area.width as usize))
                .dim()
                .render(area, buf);
            let available = area.width.saturating_sub(2) as usize;
            if available == 0 {
                return;
            }
            let (prefix, _, prefix_width) = take_prefix_by_width(&self.title, available);
            let title = if prefix_width < display_width(&self.title) {
                let (short_prefix, _, _) =
                    take_prefix_by_width(&self.title, available.saturating_sub(display_width("…")));
                format!("{short_prefix}…")
            } else {
                prefix
            };
            format!(" {title} ").dim().render(area, buf);
        } else {
            Span::from("/ ".repeat(area.width as usize / 2))
                .dim()
                .render(area, buf);
            let header = format!("/ {}", self.title);
            header.dim().render(area, buf);
        }
    }

    fn render_content(&self, area: Rect, buf: &mut Buffer) {
        let mut y = -(self.scroll_offset as isize);
        let mut drawn_bottom = area.y;
        for renderable in &self.renderables {
            let top = y;
            let height = renderable.desired_height(area.width) as isize;
            y += height;
            let bottom = y;
            if bottom < area.y as isize {
                continue;
            }
            if top > area.y as isize + area.height as isize {
                break;
            }
            if top < 0 {
                let drawn = render_offset_content(area, buf, &**renderable, (-top) as u16);
                drawn_bottom = drawn_bottom.max(area.y + drawn);
            } else {
                let draw_height = (height as u16).min(area.height.saturating_sub(top as u16));
                let draw_area = Rect::new(area.x, area.y + top as u16, area.width, draw_height);
                renderable.render(draw_area, buf);
                drawn_bottom = drawn_bottom.max(draw_area.y.saturating_add(draw_area.height));
            }
        }

        for y in drawn_bottom..area.bottom() {
            if area.width == 0 {
                break;
            }
            buf[(area.x, y)] = Cell::from('~');
            for x in area.x + 1..area.right() {
                buf[(x, y)] = Cell::from(' ');
            }
        }
    }

    fn render_bottom_bar(
        &self,
        full_area: Rect,
        content_area: Rect,
        buf: &mut Buffer,
        total_len: usize,
    ) {
        let sep_y = content_area.bottom();
        let sep_rect = Rect::new(full_area.x, sep_y, full_area.width, 1);

        Span::from("─".repeat(sep_rect.width as usize))
            .dim()
            .render(sep_rect, buf);
        if !self.scroll_percentage_visible {
            return;
        }
        let percent = if total_len == 0 {
            100
        } else {
            let max_scroll = total_len.saturating_sub(content_area.height as usize);
            if max_scroll == 0 {
                100
            } else {
                (((self.scroll_offset.min(max_scroll)) as f32 / max_scroll as f32) * 100.0).round()
                    as u8
            }
        };
        let pct_text = format!(" {percent}% ");
        let pct_w = pct_text.chars().count() as u16;
        let pct_x = sep_rect.x + sep_rect.width - pct_w - 1;
        Span::from(pct_text)
            .dim()
            .render(Rect::new(pct_x, sep_rect.y, pct_w, 1), buf);
    }

    fn handle_key_event(&mut self, tui: &mut tui::Tui, key_event: KeyEvent) -> Result<()> {
        let viewport_area = tui.terminal.viewport_area;
        self.handle_key_event_with_viewport(
            tui,
            key_event,
            viewport_area,
            self.page_height(viewport_area),
        )
    }

    /// Handles navigation for a caller that renders the pager into a sub-area of the terminal.
    ///
    /// The workspace must use the current sub-area height rather than the last full-terminal
    /// height because the composer can change size before the next draw.
    fn handle_key_event_in_area(
        &mut self,
        tui: &mut tui::Tui,
        key_event: KeyEvent,
        viewport_area: Rect,
    ) -> Result<()> {
        let page_height = self.content_area(viewport_area).height as usize;
        self.handle_key_event_with_viewport(tui, key_event, viewport_area, page_height)
    }

    fn handle_key_event_with_viewport(
        &mut self,
        tui: &mut tui::Tui,
        key_event: KeyEvent,
        viewport_area: Rect,
        page_height: usize,
    ) -> Result<()> {
        match key_event {
            e if self.keymap.scroll_up.is_pressed(e) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
            }
            e if self.keymap.scroll_down.is_pressed(e) => {
                self.scroll_offset = self.scroll_offset.saturating_add(1);
            }
            e if self.keymap.page_up.is_pressed(e) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(page_height);
            }
            e if self.keymap.page_down.is_pressed(e) => {
                self.scroll_offset = self.scroll_offset.saturating_add(page_height);
            }
            e if self.keymap.half_page_down.is_pressed(e) => {
                let area = self.content_area(viewport_area);
                let half_page = (area.height as usize).saturating_add(1) / 2;
                self.scroll_offset = self.scroll_offset.saturating_add(half_page);
            }
            e if self.keymap.half_page_up.is_pressed(e) => {
                let area = self.content_area(viewport_area);
                let half_page = (area.height as usize).saturating_add(1) / 2;
                self.scroll_offset = self.scroll_offset.saturating_sub(half_page);
            }
            e if self.keymap.jump_top.is_pressed(e) => {
                self.scroll_offset = 0;
            }
            e if self.keymap.jump_bottom.is_pressed(e) => {
                self.scroll_offset = usize::MAX;
            }
            _ => {
                return Ok(());
            }
        }
        tui.frame_requester()
            .schedule_frame_in(crate::tui::TARGET_FRAME_INTERVAL);
        Ok(())
    }

    /// Returns the height of one page in content rows.
    ///
    /// Prefers the last rendered content height (excluding header/footer chrome);
    /// if no render has occurred yet, falls back to the content area height
    /// computed from the given viewport.
    fn page_height(&self, viewport_area: Rect) -> usize {
        self.last_content_height
            .unwrap_or_else(|| self.content_area(viewport_area).height as usize)
    }

    fn update_last_content_height(&mut self, height: u16) {
        self.last_content_height = Some(height as usize);
    }

    fn content_area(&self, area: Rect) -> Rect {
        let mut area = area;
        area.y = area.y.saturating_add(1);
        area.height = area.height.saturating_sub(2);
        area
    }
}

impl PagerView {
    fn is_scrolled_to_bottom(&self) -> bool {
        if self.scroll_offset == usize::MAX {
            return true;
        }
        let Some(height) = self.last_content_height else {
            return false;
        };
        if self.renderables.is_empty() {
            return true;
        }
        let Some(total_height) = self.last_rendered_height else {
            return false;
        };
        if total_height <= height {
            return true;
        }
        let max_scroll = total_height.saturating_sub(height);
        self.scroll_offset >= max_scroll
    }

    /// Request that the given text chunk index be scrolled into view on next render.
    fn scroll_chunk_into_view(&mut self, chunk_index: usize) {
        self.pending_scroll_chunk = Some(chunk_index);
    }

    fn ensure_chunk_visible(&mut self, idx: usize, area: Rect) {
        if area.height == 0 || idx >= self.renderables.len() {
            return;
        }
        let first = self
            .renderables
            .iter()
            .take(idx)
            .map(|r| r.desired_height(area.width) as usize)
            .sum();
        let last = first + self.renderables[idx].desired_height(area.width) as usize;
        let current_top = self.scroll_offset;
        let current_bottom = current_top.saturating_add(area.height.saturating_sub(1) as usize);
        if first < current_top {
            self.scroll_offset = first;
        } else if last > current_bottom {
            self.scroll_offset = last.saturating_sub(area.height.saturating_sub(1) as usize);
        }
    }
}

/// A renderable that caches its desired height.
struct CachedRenderable {
    renderable: Box<dyn Renderable>,
    height: std::cell::Cell<Option<u16>>,
    last_width: std::cell::Cell<Option<u16>>,
}

impl CachedRenderable {
    fn new(renderable: impl Into<Box<dyn Renderable>>) -> Self {
        Self {
            renderable: renderable.into(),
            height: std::cell::Cell::new(None),
            last_width: std::cell::Cell::new(None),
        }
    }
}

impl Renderable for CachedRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.renderable.render(area, buf);
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        self.renderable.render_scrolled(area, buf, scroll_offset)
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.last_width.get() != Some(width) {
            let height = self.renderable.desired_height(width);
            self.height.set(Some(height));
            self.last_width.set(Some(width));
        }
        self.height.get().unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum TranscriptHistoryState {
    #[default]
    Idle,
    LoadingOlder,
    LoadingBeginning,
    Partial,
    Failed,
    Complete,
}

impl TranscriptHistoryState {
    fn has_unloaded_history(self) -> bool {
        matches!(
            self,
            Self::LoadingOlder | Self::LoadingBeginning | Self::Partial | Self::Failed
        )
    }

    fn session_header_placeholder(self) -> Option<&'static str> {
        match self {
            Self::LoadingOlder | Self::LoadingBeginning => Some("Loading earlier messages..."),
            Self::Partial => Some("Earlier messages are available — scroll up to load them"),
            Self::Failed => Some("Earlier messages unavailable — scroll up to retry"),
            Self::Idle | Self::Complete => None,
        }
    }
}

pub(crate) struct TranscriptOverlay {
    /// Pager UI state and the renderables currently displayed.
    ///
    /// The committed cells are rendered directly, except that collapsed workspace turns replace
    /// their hidden cells with one summary row.
    view: PagerView,
    /// Committed transcript cells (does not include the live tail).
    cells: Vec<Arc<dyn HistoryCell>>,
    highlight_cell: Option<usize>,
    /// Cache key for the render-only live tail appended after committed cells.
    live_tail_key: Option<LiveTailKey>,
    live_tail_present: bool,
    history_state: TranscriptHistoryState,
    is_done: bool,
    mode: TranscriptMode,
    workspace_turns: TranscriptTurnState,
    workspace_target_mode: WorkspaceTargetMode,
    local_image_previews_enabled: bool,
    workspace_target: Rc<TargetCell<Option<usize>>>,
    /// Width-keyed physical layout used for O(log n) turn targeting while scrolling.
    workspace_layout_index: Option<Box<WorkspaceLayoutIndex>>,
}

const WORKSPACE_WHEEL_SCROLL_ROWS: usize = 3;

/// Cache key for the active-cell "live tail" appended to the transcript overlay.
///
/// Changing any field implies a different rendered tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LiveTailKey {
    /// Current terminal width, which affects wrapping.
    width: u16,
    /// Revision that changes on in-place active cell transcript updates.
    revision: u64,
    /// Whether the tail should be treated as a continuation for spacing.
    is_stream_continuation: bool,
    /// Optional animation tick to refresh spinners/progress indicators.
    animation_tick: Option<u64>,
}

impl TranscriptOverlay {
    /// Creates a transcript overlay for a fixed set of committed cells.
    ///
    /// This overlay does not own the "active cell"; callers may optionally append a live tail via
    /// `sync_live_tail` during draws to reflect in-flight activity.
    pub(crate) fn new(transcript_cells: Vec<Arc<dyn HistoryCell>>, keymap: PagerKeymap) -> Self {
        Self::with_mode(transcript_cells, keymap, TranscriptMode::Viewer)
    }

    pub(crate) fn new_workspace(
        transcript_cells: Vec<Arc<dyn HistoryCell>>,
        keymap: PagerKeymap,
    ) -> Self {
        Self::with_mode(transcript_cells, keymap, TranscriptMode::Workspace)
    }

    fn with_mode(
        transcript_cells: Vec<Arc<dyn HistoryCell>>,
        keymap: PagerKeymap,
        mode: TranscriptMode,
    ) -> Self {
        let workspace_turns = if mode.is_workspace() {
            TranscriptTurnState::new(&transcript_cells)
        } else {
            Default::default()
        };
        let workspace_target = Rc::new(TargetCell::new(workspace_turns.selected_turn_start()));
        let mut view = PagerView::new(
            Self::render_cells(
                &transcript_cells,
                /*highlight_cell*/ None,
                TranscriptHistoryState::Idle,
                &workspace_turns,
                /*local_image_previews_enabled*/ false,
                mode.is_workspace(),
                workspace_target.clone(),
            ),
            mode.title().to_string(),
            usize::MAX,
            keymap,
        );
        view.workspace_header = mode.is_workspace();
        Self {
            view,
            cells: transcript_cells,
            highlight_cell: None,
            live_tail_key: None,
            live_tail_present: false,
            history_state: TranscriptHistoryState::Idle,
            is_done: false,
            mode,
            workspace_turns,
            workspace_target_mode: WorkspaceTargetMode::FollowViewport,
            local_image_previews_enabled: false,
            workspace_target,
            workspace_layout_index: None,
        }
    }

    pub(crate) fn is_workspace(&self) -> bool {
        self.mode.is_workspace()
    }

    pub(crate) fn set_local_image_previews_enabled(&mut self, enabled: bool) {
        if self.local_image_previews_enabled == enabled {
            return;
        }
        self.local_image_previews_enabled = enabled;
        let live_tail = self.take_live_tail_renderable();
        self.rebuild_renderables(live_tail);
    }

    pub(crate) fn render_workspace(&mut self, area: Rect, buf: &mut Buffer) {
        self.ensure_workspace_layout_index(area.width);
        self.view.render(area, buf);
    }

    /// Rebuild the Workspace-only projection after external cell state changes.
    ///
    /// Candidate cells resolve their compact form from the shared skill catalog, so their visible
    /// height can change without replacing the underlying history cell. Keep the viewport offset
    /// and selected turn intact while discarding both the renderable and layout caches.
    pub(crate) fn invalidate_workspace_catalog_view(&mut self) {
        if !self.is_workspace() {
            return;
        }
        let live_tail = self.take_live_tail_renderable();
        self.rebuild_renderables(live_tail);
    }

    fn invalidate_workspace_layout(&mut self) {
        if self.is_workspace() {
            self.workspace_layout_index = None;
        }
    }

    fn ensure_workspace_layout_index(&mut self, width: u16) {
        if !self.is_workspace() {
            return;
        }
        let width = width.max(1);
        if self
            .workspace_layout_index
            .as_ref()
            .is_some_and(|index| index.width == width)
        {
            return;
        }
        self.workspace_layout_index = Some(Box::new(self.build_workspace_layout_index(width)));
        // The cached pager height may belong to a previous width or fold state. Until the next
        // draw recomputes the live-tail-inclusive height, use the freshly built committed layout.
        self.view.last_rendered_height = None;
    }

    fn build_workspace_layout_index(&self, width: u16) -> WorkspaceLayoutIndex {
        let mut top = 0usize;
        let mut cells = Vec::with_capacity(self.cells.len());
        let mut turns = Vec::new();
        let mut active_turn = None;

        for (index, cell) in self.cells.iter().enumerate() {
            if self.workspace_turns.is_cell_hidden(index) {
                continue;
            }
            if cell.as_any().is::<UserHistoryCell>() {
                if let Some((turn_start, turn_top)) = active_turn.take() {
                    turns.push(WorkspaceTurnLayout {
                        turn_start,
                        top: turn_top,
                        bottom: top,
                    });
                }
                active_turn = Some((index, top));
            }

            let base_height = self.workspace_cell_base_height(index, width);
            let preview_rows = Self::image_preview_rows(
                cell,
                index,
                &self.workspace_turns,
                self.local_image_previews_enabled,
            );
            cells.push(WorkspaceCellLayout {
                cell_index: index,
                top,
                base_height,
                preview_rows,
            });
            top = top.saturating_add(base_height + usize::from(preview_rows));
            if self.workspace_turns.is_collapsed(index) {
                top = top.saturating_add(1);
            }
        }

        if let Some((turn_start, turn_top)) = active_turn {
            turns.push(WorkspaceTurnLayout {
                turn_start,
                top: turn_top,
                bottom: top,
            });
        }

        WorkspaceLayoutIndex {
            width,
            cells,
            turns,
            total_height: top,
        }
    }

    fn workspace_cell_base_height(&self, index: usize, width: u16) -> usize {
        let cell = &self.cells[index];
        let placeholder = cell.as_any().is::<SessionInfoCell>()
            && self.history_state.session_header_placeholder().is_some();
        let height = if placeholder {
            1
        } else {
            cell.desired_workspace_transcript_height(width) as usize
        };
        // A loading placeholder is rendered as a bare one-line renderable in
        // `render_cell`, so it does not receive the normal inter-cell inset.
        if placeholder {
            return height;
        }
        let inset = if !cell.is_stream_continuation() && index > 0 {
            1
        } else {
            0
        };
        height.saturating_add(inset)
    }

    pub(crate) fn handle_workspace_key(
        &mut self,
        tui: &mut tui::Tui,
        key_event: KeyEvent,
        transcript_area: Rect,
    ) -> Result<bool> {
        if !self.is_workspace() {
            return Ok(false);
        }
        if self.view.keymap.close_transcript.is_pressed(key_event) {
            self.is_done = true;
            return Ok(true);
        }
        let turn_shortcut = key_event.modifiers == crossterm::event::KeyModifiers::ALT
            && matches!(
                key_event.code,
                KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right
            );
        if turn_shortcut {
            let changed = match key_event.code {
                KeyCode::Up => self.workspace_turns.select_previous(),
                KeyCode::Down => self.workspace_turns.select_next(),
                KeyCode::Left => self.workspace_turns.collapse_selected(&self.cells),
                KeyCode::Right => self.workspace_turns.expand_selected(),
                _ => false,
            };
            if changed {
                if matches!(key_event.code, KeyCode::Up | KeyCode::Down) {
                    self.workspace_target_mode = WorkspaceTargetMode::Manual;
                }
                self.rebuild_workspace_turn_renderables();
                tui.frame_requester()
                    .schedule_frame_in(crate::tui::TARGET_FRAME_INTERVAL);
            }
            return Ok(true);
        }
        if self.workspace_navigation_key(key_event) {
            self.view
                .handle_key_event_in_area(tui, key_event, transcript_area)?;
            self.sync_workspace_turn_to_viewport(transcript_area);
            return Ok(true);
        }
        Ok(false)
    }

    /// Consume a mouse wheel event without letting it reach the fixed composer.
    pub(crate) fn handle_workspace_mouse(
        &mut self,
        tui: &mut tui::Tui,
        mouse_event: MouseEvent,
        transcript_area: Rect,
    ) -> Result<bool> {
        if !self.is_workspace() {
            return Ok(false);
        }
        match mouse_event.kind {
            MouseEventKind::ScrollUp => {
                self.view.scroll_offset = self
                    .view
                    .scroll_offset
                    .saturating_sub(WORKSPACE_WHEEL_SCROLL_ROWS);
            }
            MouseEventKind::ScrollDown => {
                self.view.scroll_offset = self
                    .view
                    .scroll_offset
                    .saturating_add(WORKSPACE_WHEEL_SCROLL_ROWS);
            }
            _ => return Ok(false),
        }
        self.sync_workspace_turn_to_viewport(transcript_area);
        tui.frame_requester()
            .schedule_frame_in(crate::tui::TARGET_FRAME_INTERVAL);
        Ok(true)
    }

    fn workspace_navigation_key(&self, key_event: KeyEvent) -> bool {
        matches!(key_event.code, KeyCode::PageUp | KeyCode::PageDown)
    }

    /// Keep the fold target aligned with the turn immediately above the
    /// workspace bottom bar after the user scrolls. This is deliberately not
    /// used for Option+Up/Down: those keys explicitly choose a different turn.
    fn sync_workspace_turn_to_viewport(&mut self, area: Rect) {
        if !self.is_workspace() {
            return;
        }
        self.workspace_target_mode = WorkspaceTargetMode::FollowViewport;
        let content = self.view.content_area(area);
        if content.width == 0 || content.height == 0 {
            return;
        }

        self.ensure_workspace_layout_index(content.width);
        let Some(index) = self.workspace_layout_index.as_ref() else {
            return;
        };
        let total_height = self.view.last_rendered_height.unwrap_or(index.total_height);
        let max_scroll = total_height.saturating_sub(content.height as usize);
        let viewport_bottom = self
            .view
            .scroll_offset
            .min(max_scroll)
            .saturating_add(content.height as usize)
            .saturating_sub(1);

        let target_turn = index.turn_at_or_before(viewport_bottom);
        if let Some(target_turn) = target_turn
            && self.workspace_turns.select_turn_start(target_turn)
        {
            self.workspace_target
                .set(self.workspace_turns.selected_turn_start());
        }
    }

    pub(crate) fn workspace_should_load_older(&self, key_event: KeyEvent) -> bool {
        self.is_workspace()
            && self.workspace_navigation_key(key_event)
            && self.should_load_older(key_event)
    }

    pub(crate) fn workspace_wheel_should_load_older(&self, mouse_event: MouseEvent) -> bool {
        self.is_workspace()
            && matches!(mouse_event.kind, MouseEventKind::ScrollUp)
            && self.view.scroll_offset
                <= self.view.last_content_height.unwrap_or(/*default*/ 0)
    }

    pub(crate) fn set_history_state(
        &mut self,
        state: TranscriptHistoryState,
    ) -> TranscriptHistoryState {
        let previous = self.history_state;
        if previous == state {
            return previous;
        }
        if previous == TranscriptHistoryState::LoadingBeginning
            && state == TranscriptHistoryState::Complete
        {
            self.view.scroll_offset = 0;
        }
        self.history_state = state;
        self.view.scroll_percentage_visible = !state.has_unloaded_history();
        if self
            .cells
            .iter()
            .any(|cell| cell.as_any().is::<SessionInfoCell>())
        {
            let live_tail = self.take_live_tail_renderable();
            self.rebuild_renderables(live_tail);
        }
        previous
    }

    fn render_cells(
        cells: &[Arc<dyn HistoryCell>],
        highlight_cell: Option<usize>,
        history_state: TranscriptHistoryState,
        workspace_turns: &TranscriptTurnState,
        local_image_previews_enabled: bool,
        workspace: bool,
        workspace_target: Rc<TargetCell<Option<usize>>>,
    ) -> Vec<Box<dyn Renderable>> {
        let highlighted_cell = workspace_turns.selected_turn_start().or(highlight_cell);
        let mut renderables = Vec::with_capacity(cells.len());
        for (index, cell) in cells.iter().enumerate() {
            if workspace_turns.is_cell_hidden(index) {
                continue;
            }
            renderables.push(Self::render_cell(
                cell,
                index,
                highlighted_cell,
                history_state,
                Self::image_preview_rows(
                    cell,
                    index,
                    workspace_turns,
                    local_image_previews_enabled,
                ),
                workspace,
                workspace_target.clone(),
            ));
            if workspace_turns.is_collapsed(index) {
                let hidden_cells = workspace_turns.hidden_cell_count_after(index, cells);
                renderables.push(Box::new(
                    Line::from(format!("  ▸ {hidden_cells} hidden transcript item(s)")).dim(),
                ));
            }
        }
        renderables
    }

    /// Build the renderable for a committed cell, caching its height when the cell is stable.
    fn render_cell(
        cell: &Arc<dyn HistoryCell>,
        index: usize,
        highlight_cell: Option<usize>,
        history_state: TranscriptHistoryState,
        image_preview_rows: u16,
        workspace: bool,
        workspace_target: Rc<TargetCell<Option<usize>>>,
    ) -> Box<dyn Renderable> {
        if cell.as_any().is::<SessionInfoCell>()
            && let Some(placeholder) = history_state.session_header_placeholder()
        {
            return Box::new(Line::from(placeholder).dim());
        }
        let cell_renderable = CellRenderable {
            cell: cell.clone(),
            cell_index: index,
            highlighted: !workspace && highlight_cell == Some(index),
            workspace,
            // User turns stay visually distinct when browsing old history;
            // the selected-turn behavior is represented by the fold target,
            // not a one-off background color.
            emphasize_user: workspace,
            workspace_target: workspace.then_some(workspace_target),
        };
        let mut cell_renderable: Box<dyn Renderable> = if cell.has_stable_transcript_height() {
            Box::new(CachedRenderable::new(cell_renderable))
        } else {
            Box::new(cell_renderable)
        };
        if !cell.is_stream_continuation() && index > 0 {
            cell_renderable = Box::new(InsetRenderable::new(
                cell_renderable,
                Insets::tlbr(
                    /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                ),
            ));
        }
        if image_preview_rows > 0 {
            cell_renderable = Box::new(ReservedBottomRenderable {
                renderable: cell_renderable,
                rows: image_preview_rows,
            });
        }
        cell_renderable
    }

    fn image_preview_rows(
        cell: &Arc<dyn HistoryCell>,
        index: usize,
        workspace_turns: &TranscriptTurnState,
        enabled: bool,
    ) -> u16 {
        if !enabled || workspace_turns.is_collapsed(index) {
            return 0;
        }
        cell.as_any()
            .downcast_ref::<UserHistoryCell>()
            .map_or(0, |cell| {
                LOCAL_IMAGE_PREVIEW_ROWS
                    .saturating_mul(u16::try_from(cell.local_image_paths.len()).unwrap_or(u16::MAX))
            })
    }

    /// Insert a committed history cell while keeping any cached live tail.
    ///
    /// The live tail is temporarily removed, the new committed cell is appended,
    /// then the tail is reattached. If the tail previously had no leading
    /// spacing because it was the only renderable, we add the missing inset
    /// when the first committed cell arrives.
    ///
    /// This expects `cell` to be a committed transcript cell (not the in-flight active cell). If
    /// the overlay was scrolled to bottom before insertion, it remains pinned to bottom after the
    /// insertion to preserve the "follow along" behavior.
    pub(crate) fn insert_cell(&mut self, cell: Arc<dyn HistoryCell>) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        let appended_user_turn = cell.as_any().is::<UserHistoryCell>();
        let tail_renderable = self.take_live_tail_renderable();
        if !self.is_workspace() {
            let had_prior_cells = !self.cells.is_empty();
            let cell_renderable = Self::render_cell(
                &cell,
                self.cells.len(),
                self.highlight_cell,
                self.history_state,
                /*image_preview_rows*/ 0,
                /*workspace*/ false,
                self.workspace_target.clone(),
            );
            self.cells.push(cell);
            self.view.renderables.push(cell_renderable);
            if let Some(tail) = tail_renderable {
                let tail = if !had_prior_cells
                    && self
                        .live_tail_key
                        .is_some_and(|key| !key.is_stream_continuation)
                {
                    Box::new(InsetRenderable::new(
                        tail,
                        Insets::tlbr(
                            /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                        ),
                    )) as Box<dyn Renderable>
                } else {
                    tail
                };
                self.view.renderables.push(tail);
                self.live_tail_present = true;
            }
            if follow_bottom {
                self.view.scroll_offset = usize::MAX;
            }
            return;
        }
        self.cells.push(cell);
        self.workspace_turns.refresh_after_append(&self.cells);
        if follow_bottom
            && appended_user_turn
            && self.workspace_target_mode == WorkspaceTargetMode::FollowViewport
            && self.workspace_turns.select_latest_turn()
        {
            self.workspace_target
                .set(self.workspace_turns.selected_turn_start());
        }
        self.rebuild_renderables(tail_renderable);
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    /// Returns whether an upward navigation is close enough to request older history.
    pub(crate) fn should_load_older(&self, key_event: KeyEvent) -> bool {
        self.should_load_from_start(key_event)
            || (self.view.scroll_offset
                <= self.view.last_content_height.unwrap_or(/*default*/ 0)
                && (self.view.keymap.scroll_up.is_pressed(key_event)
                    || self.view.keymap.page_up.is_pressed(key_event)
                    || self.view.keymap.half_page_up.is_pressed(key_event)))
    }

    pub(crate) fn should_load_from_start(&self, key_event: KeyEvent) -> bool {
        self.view.keymap.jump_top.is_pressed(key_event)
    }

    /// Prepends history without moving visible content and returns its insertion index.
    pub(crate) fn prepend(&mut self, cells: Vec<Arc<dyn HistoryCell>>, width: u16) -> usize {
        if cells.is_empty() {
            return 0;
        }
        let follow_bottom = self.view.is_scrolled_to_bottom();
        let previous_height = self.view.content_height(width);
        let live_tail = self.take_live_tail_renderable();
        let added_cells = cells.len();
        let insert_at = self
            .cells
            .iter()
            .rposition(|cell| cell.as_any().is::<SessionInfoCell>())
            .map_or(/*default*/ 0, |index| index.saturating_add(/*rhs*/ 1));
        self.cells.splice(insert_at..insert_at, cells);
        if self.is_workspace() {
            self.workspace_turns
                .shift_indices_from(insert_at, added_cells);
            self.workspace_turns.refresh_after_append(&self.cells);
        }
        for index in [
            &mut self.highlight_cell,
            &mut self.view.pending_scroll_chunk,
        ] {
            if let Some(index) = index.as_mut()
                && *index >= insert_at
            {
                *index = index.saturating_add(added_cells);
            }
        }
        self.rebuild_renderables(live_tail);
        let content_height = self.view.content_height(width);
        self.view.scroll_offset = if follow_bottom {
            usize::MAX
        } else {
            self.view
                .scroll_offset
                .saturating_add(content_height.saturating_sub(previous_height))
        };
        self.view.last_rendered_height = Some(content_height);
        insert_at
    }

    /// Replace committed transcript cells while keeping any cached in-progress output that is
    /// currently shown at the end of the overlay.
    ///
    /// This is used when existing history is trimmed (for example after rollback) so the
    /// transcript overlay immediately reflects the same committed cells as the main transcript.
    pub(crate) fn replace_cells(&mut self, cells: Vec<Arc<dyn HistoryCell>>) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        let live_tail = self.take_live_tail_renderable();
        self.cells = cells;
        if self.is_workspace() {
            self.workspace_turns.reset(&self.cells);
            self.workspace_target_mode = WorkspaceTargetMode::FollowViewport;
        }
        if self
            .highlight_cell
            .is_some_and(|idx| idx >= self.cells.len())
        {
            self.highlight_cell = None;
        }
        self.rebuild_renderables(live_tail);
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    /// Replace a range of committed cells with a single consolidated cell.
    ///
    /// Mirrors the splice performed on `App::transcript_cells` during
    /// `ConsolidateAgentMessage` so the Ctrl+T overlay stays in sync with the
    /// main transcript. The range is clamped defensively: cells may have been
    /// inserted after the overlay opened, leaving it with fewer entries than
    /// the main transcript.
    pub(crate) fn consolidate_cells(
        &mut self,
        range: std::ops::Range<usize>,
        consolidated: Arc<dyn HistoryCell>,
    ) {
        let follow_bottom = self.view.is_scrolled_to_bottom();
        // Clamp the range to the overlay's cell count to avoid panic if the overlay has fewer
        // cells than the main transcript (e.g. cells were inserted after the overlay has opened).
        let clamped_end = range.end.min(self.cells.len());
        let clamped_start = range.start.min(clamped_end);
        if clamped_start < clamped_end {
            let live_tail = self.take_live_tail_renderable();
            let removed = clamped_end - clamped_start;
            if let Some(highlight_cell) = self.highlight_cell.as_mut()
                && *highlight_cell >= clamped_start
            {
                if *highlight_cell < clamped_end {
                    *highlight_cell = clamped_start;
                } else {
                    *highlight_cell = highlight_cell.saturating_sub(removed.saturating_sub(1));
                }
            }
            self.cells
                .splice(clamped_start..clamped_end, std::iter::once(consolidated));
            if self.is_workspace() {
                self.workspace_turns.reset(&self.cells);
                self.workspace_target_mode = WorkspaceTargetMode::FollowViewport;
            }
            if self
                .highlight_cell
                .is_some_and(|highlight_cell| highlight_cell >= self.cells.len())
            {
                self.highlight_cell = None;
            }
            self.rebuild_renderables(live_tail);
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    /// Sync the active-cell live tail with the current width and cell state.
    ///
    /// Recomputes the tail only when the cache key changes, preserving scroll
    /// position and dropping the tail if there is nothing to render.
    ///
    /// The overlay owns committed transcript cells while the live tail is derived from the current
    /// active cell, which can mutate in place while streaming. `App` calls this during
    /// `TuiEvent::Draw` for `Overlay::Transcript`, passing a key that changes when the active cell
    /// mutates or animates so the cached tail stays fresh.
    ///
    /// Passing a key that does not change on in-place active-cell mutations will freeze the tail in
    /// `Ctrl+T` while the main viewport continues to update.
    pub(crate) fn sync_live_tail(
        &mut self,
        width: u16,
        active_key: Option<ActiveCellTranscriptKey>,
        compute_lines: impl FnOnce(u16) -> Option<Vec<HyperlinkLine>>,
    ) {
        let next_key = active_key.map(|key| LiveTailKey {
            width,
            revision: key.revision,
            is_stream_continuation: key.is_stream_continuation,
            animation_tick: key.animation_tick,
        });

        if self.live_tail_key == next_key {
            return;
        }
        let follow_bottom = self.view.is_scrolled_to_bottom();

        self.take_live_tail_renderable();
        self.live_tail_key = next_key;
        self.live_tail_present = false;
        self.invalidate_workspace_layout();

        if let Some(key) = next_key {
            let lines = compute_lines(width).unwrap_or_default();
            if !lines.is_empty() {
                self.view.renderables.push(Self::live_tail_renderable(
                    lines,
                    !self.cells.is_empty(),
                    key.is_stream_continuation,
                ));
                self.live_tail_present = true;
            }
        }
        if follow_bottom {
            self.view.scroll_offset = usize::MAX;
        }
    }

    pub(crate) fn set_highlight_cell(&mut self, cell: Option<usize>) {
        let previous = self.highlight_cell;
        self.highlight_cell = cell;
        if previous != cell {
            if self.is_workspace() {
                let live_tail = self.take_live_tail_renderable();
                self.rebuild_renderables(live_tail);
            } else {
                for index in [previous, cell].into_iter().flatten() {
                    if let Some(history_cell) = self.cells.get(index) {
                        self.view.renderables[index] = Self::render_cell(
                            history_cell,
                            index,
                            self.highlight_cell,
                            self.history_state,
                            /*image_preview_rows*/ 0,
                            /*workspace*/ false,
                            self.workspace_target.clone(),
                        );
                    }
                }
            }
        }
        if let Some(idx) = self.highlight_cell {
            self.view.scroll_chunk_into_view(idx);
        }
    }

    /// Returns whether the underlying pager view is currently pinned to the bottom.
    ///
    /// The `App` draw loop uses this to decide whether to schedule animation frames for the live
    /// tail; if the user has scrolled up, we avoid driving animation work that they cannot see.
    pub(crate) fn is_scrolled_to_bottom(&self) -> bool {
        self.view.is_scrolled_to_bottom()
    }

    // Detach the live tail before changing cells: their old count identifies the tail renderable.
    fn rebuild_renderables(&mut self, tail_renderable: Option<Box<dyn Renderable>>) {
        self.invalidate_workspace_layout();
        if self.is_workspace() {
            self.workspace_target
                .set(self.workspace_turns.selected_turn_start());
        }
        self.view.renderables = Self::render_cells(
            &self.cells,
            self.highlight_cell,
            self.history_state,
            &self.workspace_turns,
            self.local_image_previews_enabled,
            self.is_workspace(),
            self.workspace_target.clone(),
        );
        if let Some(tail) = tail_renderable {
            self.view.renderables.push(tail);
            self.live_tail_present = true;
        }
    }

    fn rebuild_workspace_turn_renderables(&mut self) {
        let live_tail = self.take_live_tail_renderable();
        self.rebuild_renderables(live_tail);
        if let Some(selected_turn) = self.workspace_turns.selected_turn_start()
            && let Some(renderable_index) = self.renderable_index_for_cell(selected_turn)
        {
            self.view.scroll_chunk_into_view(renderable_index);
        }
    }

    fn renderable_index_for_cell(&self, cell_index: usize) -> Option<usize> {
        let mut renderable_index = 0usize;
        for (index, _) in self.cells.iter().enumerate() {
            if self.workspace_turns.is_cell_hidden(index) {
                continue;
            }
            if index == cell_index {
                return Some(renderable_index);
            }
            renderable_index = renderable_index.saturating_add(1);
            if self.workspace_turns.is_collapsed(index) {
                renderable_index = renderable_index.saturating_add(1);
            }
        }
        None
    }

    pub(crate) fn workspace_local_image_previews(
        &mut self,
        area: Rect,
    ) -> Vec<crate::pets::LocalImagePreviewDraw> {
        if !self.is_workspace() || !self.local_image_previews_enabled {
            return Vec::new();
        }
        let content = self.view.content_area(area);
        let columns = content
            .width
            .saturating_sub(/*left and right gutter*/ 4)
            .min(LOCAL_IMAGE_PREVIEW_COLUMNS);
        if columns == 0 || content.height < LOCAL_IMAGE_PREVIEW_ROWS {
            return Vec::new();
        }
        self.ensure_workspace_layout_index(content.width);
        let Some(index) = self.workspace_layout_index.as_ref() else {
            return Vec::new();
        };
        let total_height = self.view.last_rendered_height.unwrap_or(index.total_height);
        let max_scroll = total_height.saturating_sub(content.height as usize);
        let scroll_offset = self.view.scroll_offset.min(max_scroll);
        let mut previews = Vec::new();
        for layout in &index.cells {
            let Some(cell) = self.cells.get(layout.cell_index) else {
                continue;
            };
            if layout.preview_rows > 0
                && let Some(user_cell) = cell.as_any().downcast_ref::<UserHistoryCell>()
            {
                for (image_index, path) in user_cell.local_image_paths.iter().enumerate() {
                    if !path.is_file() {
                        continue;
                    }
                    let image_top = content.y as isize
                        + layout.top as isize
                        + layout.base_height as isize
                        + (image_index.saturating_mul(usize::from(LOCAL_IMAGE_PREVIEW_ROWS))
                            as isize);
                    let image_top = image_top.saturating_sub(scroll_offset as isize);
                    let image_bottom = image_top.saturating_add(LOCAL_IMAGE_PREVIEW_ROWS as isize);
                    if image_top >= content.y as isize && image_bottom <= content.bottom() as isize
                    {
                        previews.push(crate::pets::LocalImagePreviewDraw {
                            image_id: 0xC100_0000u32
                                .saturating_add((layout.cell_index as u32).saturating_mul(16))
                                .saturating_add(image_index as u32),
                            path: path.clone(),
                            x: content.x.saturating_add(2),
                            y: image_top as u16,
                            columns,
                            rows: LOCAL_IMAGE_PREVIEW_ROWS,
                        });
                    }
                }
            }
        }
        previews
    }

    /// Removes and returns the cached live-tail renderable, if present.
    ///
    /// The live tail is represented as a single optional renderable appended after the committed
    /// cell renderables, so this relies on the live tail always being the final entry in
    /// `view.renderables` when present.
    fn take_live_tail_renderable(&mut self) -> Option<Box<dyn Renderable>> {
        if !std::mem::take(&mut self.live_tail_present) {
            return None;
        }
        self.view.renderables.pop()
    }

    fn live_tail_renderable(
        lines: Vec<HyperlinkLine>,
        has_prior_cells: bool,
        is_stream_continuation: bool,
    ) -> Box<dyn Renderable> {
        let mut renderable: Box<dyn Renderable> =
            Box::new(CachedRenderable::new(HyperlinkLinesRenderable { lines }));
        if has_prior_cells && !is_stream_continuation {
            renderable = Box::new(InsetRenderable::new(
                renderable,
                Insets::tlbr(
                    /*top*/ 1, /*left*/ 0, /*bottom*/ 0, /*right*/ 0,
                ),
            ));
        }
        renderable
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_navigation_hints(line1, buf, &self.view.keymap);

        let mut pairs: Vec<(Vec<ShortcutHint>, &str)> = vec![(
            first_or_empty(&self.view.keymap, "close", &self.view.keymap.close),
            "close",
        )];
        if self.highlight_cell.is_some() {
            pairs.push((
                vec![
                    key_hint::plain(KeyCode::Esc).into(),
                    key_hint::plain(KeyCode::Left).into(),
                ],
                "to edit prev",
            ));
            pairs.push((vec![key_hint::plain(KeyCode::Right).into()], "to edit next"));
            pairs.push((
                vec![key_hint::plain(KeyCode::Enter).into()],
                "to edit message",
            ));
        } else {
            pairs.push((vec![key_hint::plain(KeyCode::Esc).into()], "to edit prev"));
        }
        render_key_hints(line2, buf, &pairs);
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // Preserve following the tail before the composer changes the available height.
        if self.view.is_scrolled_to_bottom() {
            self.view.scroll_offset = usize::MAX;
        }
        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let bottom = Rect::new(area.x, area.y + top_h, area.width, 3);
        self.view.render(top, buf);
        self.render_history_state(top, buf);
        self.render_hints(bottom, buf);
    }

    fn render_history_state(&self, area: Rect, buf: &mut Buffer) {
        if area.height == 0 {
            return;
        }
        let label = match self.history_state {
            TranscriptHistoryState::Idle => return,
            TranscriptHistoryState::LoadingOlder | TranscriptHistoryState::LoadingBeginning => {
                " loading older history... "
            }
            TranscriptHistoryState::Partial => " partial history | PgUp for earlier ",
            TranscriptHistoryState::Failed => " history unavailable | PgUp to retry ",
            TranscriptHistoryState::Complete => " start of history ",
        };
        let width = (label.chars().count() as u16).min(area.width);
        let status_area = Rect::new(
            area.right().saturating_sub(width),
            area.y,
            width,
            /*height*/ 1,
        );
        Span::from(label).dim().render(status_area, buf);
    }
}

/// Reserves blank rows for a terminal graphic while preserving viewport-aware cell rendering.
struct ReservedBottomRenderable {
    renderable: Box<dyn Renderable>,
    rows: u16,
}

impl Renderable for ReservedBottomRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        let content_height = self.renderable.desired_height(area.width);
        self.renderable.render(
            Rect::new(area.x, area.y, area.width, area.height.min(content_height)),
            buf,
        );
    }

    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        let content_height = self.renderable.desired_height(area.width);
        if scroll_offset >= content_height {
            return true;
        }
        self.renderable.render_scrolled(
            Rect::new(
                area.x,
                area.y,
                area.width,
                area.height
                    .min(content_height.saturating_sub(scroll_offset)),
            ),
            buf,
            scroll_offset,
        )
    }

    fn desired_height(&self, width: u16) -> u16 {
        self.renderable
            .desired_height(width)
            .saturating_add(self.rows)
    }
}

impl TranscriptOverlay {
    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key_event) => match key_event {
                e if self.view.keymap.close.is_pressed(e)
                    || self.view.keymap.close_transcript.is_pressed(e) =>
                {
                    self.is_done = true;
                    Ok(())
                }
                other => self.view.handle_key_event(tui, other),
            },
            TuiEvent::Draw | TuiEvent::Resume | TuiEvent::Resize(_) | TuiEvent::FocusGained => {
                tui.draw(u16::MAX, |frame| {
                    self.render(frame.area(), frame.buffer);
                })?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
}

pub(crate) struct StaticOverlay {
    view: PagerView,
    is_done: bool,
}

impl StaticOverlay {
    pub(crate) fn with_title(
        lines: Vec<Line<'static>>,
        title: String,
        keymap: PagerKeymap,
    ) -> Self {
        let paragraph = Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false });
        Self::with_renderables(
            vec![Box::new(CachedRenderable::new(paragraph))],
            title,
            keymap,
        )
    }

    pub(crate) fn with_renderables(
        renderables: Vec<Box<dyn Renderable>>,
        title: String,
        keymap: PagerKeymap,
    ) -> Self {
        Self {
            view: PagerView::new(renderables, title, /*scroll_offset*/ 0, keymap),
            is_done: false,
        }
    }

    fn render_hints(&self, area: Rect, buf: &mut Buffer) {
        let line1 = Rect::new(area.x, area.y, area.width, 1);
        let line2 = Rect::new(area.x, area.y.saturating_add(1), area.width, 1);
        render_navigation_hints(line1, buf, &self.view.keymap);
        let pairs: Vec<(Vec<ShortcutHint>, &str)> = vec![(
            first_or_empty(&self.view.keymap, "close", &self.view.keymap.close),
            "close",
        )];
        render_key_hints(line2, buf, &pairs);
    }

    pub(crate) fn render(&mut self, area: Rect, buf: &mut Buffer) {
        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let bottom = Rect::new(area.x, area.y + top_h, area.width, 3);
        self.view.render(top, buf);
        self.render_hints(bottom, buf);
    }
}

impl StaticOverlay {
    pub(crate) fn handle_event(&mut self, tui: &mut tui::Tui, event: TuiEvent) -> Result<()> {
        match event {
            TuiEvent::Key(key_event) => match key_event {
                e if self.view.keymap.close.is_pressed(e) => {
                    self.is_done = true;
                    Ok(())
                }
                other => self.view.handle_key_event(tui, other),
            },
            TuiEvent::Draw | TuiEvent::Resume | TuiEvent::Resize(_) | TuiEvent::FocusGained => {
                tui.draw(u16::MAX, |frame| {
                    self.render(frame.area(), frame.buffer);
                })?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
    pub(crate) fn is_done(&self) -> bool {
        self.is_done
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history_cell::ReviewDecision;
    use codex_app_server_protocol::CommandExecutionSource as ExecCommandSource;
    use insta::assert_snapshot;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use crate::diff_model::FileChange;
    use crate::exec_cell::CommandOutput;
    use crate::history_cell;
    use crate::history_cell::HistoryCell;
    use crate::history_cell::new_patch_event;
    use codex_protocol::parse_command::ParsedCommand;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::text::Text;

    #[derive(Debug)]
    struct TestCell {
        lines: Vec<Line<'static>>,
    }

    impl crate::history_cell::HistoryCell for TestCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            self.lines.clone()
        }

        fn raw_lines(&self) -> Vec<Line<'static>> {
            self.lines.clone()
        }

        fn transcript_lines(&self, _width: u16) -> Vec<Line<'static>> {
            self.lines.clone()
        }
    }

    #[derive(Debug)]
    struct HeightCountingCell {
        height_calls: Arc<AtomicUsize>,
    }

    impl crate::history_cell::HistoryCell for HeightCountingCell {
        fn display_lines(&self, _width: u16) -> Vec<Line<'static>> {
            vec![Line::from("counted")]
        }

        fn raw_lines(&self) -> Vec<Line<'static>> {
            vec![Line::from("counted")]
        }

        fn desired_transcript_height(&self, _width: u16) -> u16 {
            self.height_calls.fetch_add(1, Ordering::Relaxed);
            1
        }
    }

    fn paragraph_block(label: &str, lines: usize) -> Box<dyn Renderable> {
        let text = Text::from(
            (0..lines)
                .map(|i| Line::from(format!("{label}{i}")))
                .collect::<Vec<_>>(),
        );
        Box::new(Paragraph::new(text)) as Box<dyn Renderable>
    }

    fn default_pager_keymap() -> crate::keymap::PagerKeymap {
        crate::keymap::RuntimeKeymap::defaults().pager
    }

    fn transcript_overlay(cells: Vec<Arc<dyn HistoryCell>>) -> TranscriptOverlay {
        TranscriptOverlay::new(cells, default_pager_keymap())
    }

    fn static_overlay(lines: Vec<Line<'static>>, title: &str) -> StaticOverlay {
        StaticOverlay::with_title(lines, title.to_string(), default_pager_keymap())
    }

    fn pager_view(
        renderables: Vec<Box<dyn Renderable>>,
        title: &str,
        scroll_offset: usize,
    ) -> PagerView {
        PagerView::new(
            renderables,
            title.to_string(),
            scroll_offset,
            default_pager_keymap(),
        )
    }

    #[test]
    fn footer_hints_display_chords_without_internal_dispatch_keys() {
        use codex_config::types::KeybindingSpec;
        use codex_config::types::KeybindingsSpec;
        use codex_config::types::TuiKeymap;

        let mut config = TuiKeymap::default();
        config.pager.page_up = Some(KeybindingsSpec::One(KeybindingSpec(
            "ctrl-x page-up".to_string(),
        )));
        let keymap = crate::keymap::RuntimeKeymap::from_config(&config).expect("valid pager chord");

        assert_eq!(
            first_or_empty(&keymap.pager, "page_up", &keymap.pager.page_up),
            vec![ShortcutHint::Chord {
                prefix: key_hint::ctrl(KeyCode::Char('x')),
                completion: key_hint::plain(KeyCode::PageUp),
            }]
        );
    }

    #[test]
    fn edit_prev_hint_is_visible() {
        let mut overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("hello")],
        })]);

        // Render into a wide buffer so the footer hints aren't truncated.
        let area = Rect::new(0, 0, 120, 10);
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        let s = buffer_to_text(&buf, area);
        assert!(
            s.contains("edit prev"),
            "expected 'edit prev' hint in overlay footer, got: {s:?}"
        );
    }

    #[test]
    fn jump_top_requests_older_history_from_the_bottom() {
        let overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("recent")],
        })]);

        let home = KeyEvent::new(KeyCode::Home, crossterm::event::KeyModifiers::NONE);

        assert!(overlay.should_load_older(home));
        assert!(overlay.should_load_from_start(home));
        assert!(!overlay.should_load_from_start(KeyEvent::new(
            KeyCode::PageUp,
            crossterm::event::KeyModifiers::NONE,
        )));
    }

    #[test]
    fn edit_next_hint_is_visible_when_highlighted() {
        let mut overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("hello")],
        })]);
        overlay.set_highlight_cell(Some(0));

        // Render into a wide buffer so the footer hints aren't truncated.
        let area = Rect::new(0, 0, 120, 10);
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        let s = buffer_to_text(&buf, area);
        assert!(
            s.contains("edit next"),
            "expected 'edit next' hint in overlay footer, got: {s:?}"
        );
    }

    #[test]
    fn transcript_overlay_snapshots_paginated_history_states() {
        let mut overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("recent transcript")],
        })]);
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 72, /*height*/ 10,
        );
        let mut snapshots = String::new();

        for (name, state) in [
            ("loading", TranscriptHistoryState::LoadingOlder),
            ("partial", TranscriptHistoryState::Partial),
            ("failed", TranscriptHistoryState::Failed),
            ("complete", TranscriptHistoryState::Complete),
        ] {
            overlay.set_history_state(state);
            let mut buf = Buffer::empty(area);
            overlay.render(area, &mut buf);
            snapshots.push_str(&format!("--- {name} ---\n{}", buffer_to_text(&buf, area)));
        }

        assert_snapshot!("transcript_overlay_paginated_history_states", snapshots);
    }

    #[test]
    fn transcript_overlay_snapshot_basic() {
        // Prepare a transcript overlay with a few lines
        let mut overlay = transcript_overlay(vec![
            Arc::new(TestCell {
                lines: vec![Line::from("alpha")],
            }),
            Arc::new(TestCell {
                lines: vec![Line::from("beta")],
            }),
            Arc::new(TestCell {
                lines: vec![Line::from("gamma")],
            }),
        ]);
        let mut term = Terminal::new(TestBackend::new(40, 10)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    #[test]
    fn transcript_overlay_preserves_semantic_web_links() {
        let destination = "https://example.com/a/very/long/path";
        let mut overlay = transcript_overlay(vec![Arc::new(history_cell::AgentMarkdownCell::new(
            destination.to_string(),
            std::path::Path::new("/tmp"),
        ))]);
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 24, /*height*/ 10,
        );
        let mut buf = Buffer::empty(area);

        overlay.render(area, &mut buf);

        assert!(area.positions().any(|position| {
            buf[position]
                .symbol()
                .contains(&format!("\x1b]8;;{destination}\x07"))
        }));
    }

    #[test]
    fn transcript_overlay_renders_live_tail() {
        let mut overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("alpha")],
        })]);
        overlay.sync_live_tail(
            /*width*/ 40,
            Some(ActiveCellTranscriptKey {
                revision: 1,
                is_stream_continuation: false,
                animation_tick: None,
            }),
            |_| Some(vec![HyperlinkLine::from("tail")]),
        );

        let mut term = Terminal::new(TestBackend::new(40, 10)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    #[test]
    fn transcript_overlay_preserves_live_tail_when_prepending_history() {
        let mut overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("recent")],
        })]);
        overlay.sync_live_tail(
            /*width*/ 40,
            Some(ActiveCellTranscriptKey {
                revision: 1,
                is_stream_continuation: false,
                animation_tick: None,
            }),
            |_| Some(vec![HyperlinkLine::from("live tail")]),
        );
        overlay.prepend(
            vec![Arc::new(TestCell {
                lines: vec![Line::from("older")],
            })],
            /*width*/ 40,
        );

        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 10,
        );
        let mut buffer = Buffer::empty(area);
        overlay.render(area, &mut buffer);
        let rendered = buffer_to_text(&buffer, area);
        assert!(rendered.contains("older"));
        assert!(rendered.contains("recent"));
        assert!(rendered.contains("live tail"));
    }

    #[test]
    fn transcript_overlay_live_tail_preserves_semantic_web_links() {
        let destination = "https://example.com/a/streamed/path";
        let cell = history_cell::AgentMarkdownCell::new(
            destination.to_string(),
            std::path::Path::new("/tmp"),
        );
        let mut overlay = transcript_overlay(Vec::new());
        let area = Rect::new(
            /*x*/ 0, /*y*/ 0, /*width*/ 24, /*height*/ 10,
        );
        let mut buf = Buffer::empty(area);

        overlay.sync_live_tail(
            area.width,
            Some(ActiveCellTranscriptKey {
                revision: 1,
                is_stream_continuation: false,
                animation_tick: None,
            }),
            |width| Some(cell.transcript_hyperlink_lines(width)),
        );
        overlay.render(area, &mut buf);

        assert!(area.positions().any(|position| {
            buf[position]
                .symbol()
                .contains(&format!("\x1b]8;;{destination}\x07"))
        }));
    }

    #[test]
    fn transcript_overlay_sync_live_tail_is_noop_for_identical_key() {
        let mut overlay = transcript_overlay(vec![Arc::new(TestCell {
            lines: vec![Line::from("alpha")],
        })]);

        let calls = std::cell::Cell::new(0usize);
        let key = ActiveCellTranscriptKey {
            revision: 1,
            is_stream_continuation: false,
            animation_tick: None,
        };

        overlay.sync_live_tail(/*width*/ 40, Some(key), |_| {
            calls.set(calls.get() + 1);
            Some(vec![HyperlinkLine::from("tail")])
        });
        overlay.sync_live_tail(/*width*/ 40, Some(key), |_| {
            calls.set(calls.get() + 1);
            Some(vec![HyperlinkLine::from("tail2")])
        });

        assert_eq!(calls.get(), 1);
    }

    fn buffer_to_text(buf: &Buffer, area: Rect) -> String {
        let mut out = String::new();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let symbol = buf[(x, y)].symbol();
                if symbol.is_empty() {
                    out.push(' ');
                } else {
                    out.push(symbol.chars().next().unwrap_or(' '));
                }
            }
            // Trim trailing spaces for stability.
            while out.ends_with(' ') {
                out.pop();
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn transcript_overlay_apply_patch_scroll_vt100_clears_previous_page() {
        let cwd = PathBuf::from("/repo");
        let mut cells: Vec<Arc<dyn HistoryCell>> = Vec::new();

        let mut approval_changes = HashMap::new();
        approval_changes.insert(
            PathBuf::from("foo.txt"),
            FileChange::Add {
                content: "hello\nworld\n".to_string(),
            },
        );
        let approval_cell: Arc<dyn HistoryCell> = Arc::new(new_patch_event(approval_changes, &cwd));
        cells.push(approval_cell);

        let mut apply_changes = HashMap::new();
        apply_changes.insert(
            PathBuf::from("foo.txt"),
            FileChange::Add {
                content: "hello\nworld\n".to_string(),
            },
        );
        let apply_begin_cell: Arc<dyn HistoryCell> = Arc::new(new_patch_event(apply_changes, &cwd));
        cells.push(apply_begin_cell);

        let apply_end_cell: Arc<dyn HistoryCell> = history_cell::new_approval_decision_cell(
            history_cell::ApprovalDecisionSubject::Command(vec!["ls".into()]),
            ReviewDecision::Approved,
            history_cell::ApprovalDecisionActor::User,
        )
        .into();
        cells.push(apply_end_cell);

        let mut exec_cell = crate::exec_cell::new_active_exec_command(
            "exec-1".into(),
            vec!["bash".into(), "-lc".into(), "ls".into()],
            vec![ParsedCommand::Unknown { cmd: "ls".into() }],
            ExecCommandSource::Agent,
            /*interaction_input*/ None,
            /*animations_enabled*/ true,
        );
        exec_cell.complete_call(
            "exec-1",
            CommandOutput::new(/*exit_code*/ 0, "src\nREADME.md\n".into()),
            Duration::from_millis(420),
        );
        let exec_cell: Arc<dyn HistoryCell> = Arc::new(exec_cell);
        cells.push(exec_cell);

        let mut overlay = transcript_overlay(cells);
        let area = Rect::new(0, 0, 80, 12);
        let mut buf = Buffer::empty(area);

        overlay.render(area, &mut buf);
        overlay.view.scroll_offset = 0;
        overlay.render(area, &mut buf);

        let snapshot = buffer_to_text(&buf, area);
        assert_snapshot!("transcript_overlay_apply_patch_scroll_vt100", snapshot);
    }

    #[test]
    fn transcript_overlay_keeps_scroll_pinned_at_bottom() {
        let mut overlay = transcript_overlay(
            (0..20)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line{i}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        let mut term = Terminal::new(TestBackend::new(40, 12)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");

        assert!(
            overlay.view.is_scrolled_to_bottom(),
            "expected initial render to leave view at bottom"
        );

        for height in [9, 14] {
            term.backend_mut().resize(/*width*/ 40, height);
            term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
                .expect("draw after composer height change");
            assert!(overlay.is_scrolled_to_bottom());
        }

        overlay.insert_cell(Arc::new(TestCell {
            lines: vec!["tail".into()],
        }));

        assert_eq!(overlay.view.scroll_offset, usize::MAX);
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw committed tail");
        assert_snapshot!("transcript_overlay_follows_resized_tail", term.backend());
    }

    #[test]
    fn transcript_overlay_preserves_manual_scroll_position() {
        let mut overlay = transcript_overlay(
            (0..20)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line{i}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        let mut term = Terminal::new(TestBackend::new(40, 12)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");

        overlay.view.scroll_offset = 0;

        overlay.insert_cell(Arc::new(TestCell {
            lines: vec!["tail".into()],
        }));

        assert_eq!(overlay.view.scroll_offset, 0);
        overlay.view.scroll_offset = 3;
        term.draw(|frame| overlay.render(frame.area(), frame.buffer_mut()))
            .expect("draw");
        let content_area = Rect::new(
            /*x*/ 0, /*y*/ 1, /*width*/ 40, /*height*/ 4,
        );
        let visible_before = buffer_to_text(term.backend().buffer(), content_area);
        overlay.prepend(
            vec![Arc::new(TestCell {
                lines: (0..40).map(|i| Line::from(format!("older {i}"))).collect(),
            })],
            /*width*/ 40,
        );
        term.draw(|frame| overlay.render(frame.area(), frame.buffer_mut()))
            .expect("draw");
        assert_eq!(
            buffer_to_text(term.backend().buffer(), content_area),
            visible_before
        );
        assert_snapshot!(
            "transcript_overlay_prepended_history",
            visible_before.trim()
        );
    }

    #[test]
    fn transcript_overlay_insert_preserves_cached_cell_heights() {
        let height_calls = Arc::new(AtomicUsize::new(0));
        let mut overlay = transcript_overlay(vec![Arc::new(HeightCountingCell {
            height_calls: height_calls.clone(),
        })]);
        let area = Rect::new(0, 0, 40, 12);
        let mut buf = Buffer::empty(area);

        overlay.render(area, &mut buf);
        assert_eq!(height_calls.load(Ordering::Relaxed), 1);

        overlay.insert_cell(Arc::new(TestCell {
            lines: vec![Line::from("inserted")],
        }));
        overlay.render(area, &mut buf);

        assert_eq!(height_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn transcript_overlay_history_rebuild_preserves_only_the_live_tail() {
        for replace in [false, true] {
            for tail in [
                None,
                Some(Vec::new()),
                Some(vec![HyperlinkLine::from("live")]),
            ] {
                let mut overlay = transcript_overlay(
                    ["first", "last"]
                        .map(|line| {
                            Arc::new(TestCell {
                                lines: vec![line.into()],
                            }) as Arc<dyn HistoryCell>
                        })
                        .to_vec(),
                );
                let key = tail.as_ref().map(|_| ActiveCellTranscriptKey {
                    revision: 1,
                    is_stream_continuation: false,
                    animation_tick: None,
                });
                overlay.sync_live_tail(/*width*/ 40, key, |_| tail.clone());
                let consolidated = Arc::new(TestCell {
                    lines: vec!["first".into(), "last".into()],
                });
                if replace {
                    overlay.replace_cells(vec![consolidated]);
                } else {
                    overlay.consolidate_cells(0..2, consolidated);
                }
                // A draw may arrive after the active tail has already been cleared.
                overlay.sync_live_tail(/*width*/ 40, key, |_| tail.clone());
                let mut reopened = transcript_overlay(overlay.cells.clone());
                reopened.sync_live_tail(/*width*/ 40, key, |_| tail.clone());
                let area = Rect::new(
                    /*x*/ 0, /*y*/ 0, /*width*/ 40, /*height*/ 10,
                );
                let mut actual = Buffer::empty(area);
                let mut expected = Buffer::empty(area);
                overlay.render(area, &mut actual);
                reopened.render(area, &mut expected);
                assert_eq!(actual, expected);
                if tail.is_none() {
                    assert_snapshot!(
                        "transcript_overlay_completed_stream",
                        buffer_to_text(&actual, area)
                    );
                }
            }
        }
    }

    #[test]
    fn transcript_overlay_consolidation_remaps_highlight_inside_range() {
        let mut overlay = transcript_overlay(
            (0..6)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line{i}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        overlay.set_highlight_cell(Some(3));

        overlay.consolidate_cells(
            2..5,
            Arc::new(TestCell {
                lines: vec![Line::from("consolidated")],
            }),
        );

        assert_eq!(
            overlay.highlight_cell,
            Some(2),
            "highlight inside consolidated range should point to replacement cell",
        );
    }

    #[test]
    fn transcript_overlay_consolidation_remaps_highlight_after_range() {
        let mut overlay = transcript_overlay(
            (0..7)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line{i}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        overlay.set_highlight_cell(Some(6));

        overlay.consolidate_cells(
            2..5,
            Arc::new(TestCell {
                lines: vec![Line::from("consolidated")],
            }),
        );

        assert_eq!(
            overlay.highlight_cell,
            Some(4),
            "highlight after consolidated range should shift left by removed cells",
        );
    }

    #[test]
    fn static_overlay_snapshot_basic() {
        // Prepare a static overlay with a few lines and a title
        let mut overlay = static_overlay(
            vec!["one".into(), "two".into(), "three".into()],
            "S T A T I C",
        );
        let mut term = Terminal::new(TestBackend::new(40, 10)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    /// Render transcript overlay and return visible line numbers (`line-NN`) in order.
    fn transcript_line_numbers(overlay: &mut TranscriptOverlay, area: Rect) -> Vec<usize> {
        let mut buf = Buffer::empty(area);
        overlay.render(area, &mut buf);

        let top_h = area.height.saturating_sub(3);
        let top = Rect::new(area.x, area.y, area.width, top_h);
        let content_area = overlay.view.content_area(top);

        let mut nums = Vec::new();
        for y in content_area.y..content_area.bottom() {
            let mut line = String::new();
            for x in content_area.x..content_area.right() {
                line.push(buf[(x, y)].symbol().chars().next().unwrap_or(' '));
            }
            if let Some(n) = line
                .split_whitespace()
                .find_map(|w| w.strip_prefix("line-"))
                .and_then(|s| s.parse().ok())
            {
                nums.push(n);
            }
        }
        nums
    }

    #[test]
    fn transcript_overlay_paging_is_continuous_and_round_trips() {
        let mut overlay = transcript_overlay(
            (0..50)
                .map(|i| {
                    Arc::new(TestCell {
                        lines: vec![Line::from(format!("line-{i:02}"))],
                    }) as Arc<dyn HistoryCell>
                })
                .collect(),
        );
        let area = Rect::new(0, 0, 40, 15);

        // Prime layout so last_content_height is populated and paging uses the real content height.
        let mut buf = Buffer::empty(area);
        overlay.view.scroll_offset = 0;
        overlay.render(area, &mut buf);
        let page_height = overlay.view.page_height(area);

        // Scenario 1: starting from the top, PageDown should show the next page of content.
        overlay.view.scroll_offset = 0;
        let page1 = transcript_line_numbers(&mut overlay, area);
        let page1_len = page1.len();
        let expected_page1: Vec<usize> = (0..page1_len).collect();
        assert_eq!(
            page1, expected_page1,
            "first page should start at line-00 and show a full page of content"
        );

        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_add(page_height);
        let page2 = transcript_line_numbers(&mut overlay, area);
        assert_eq!(
            page2.len(),
            page1_len,
            "second page should have the same number of visible lines as the first page"
        );
        let expected_page2_first = *page1.last().unwrap() + 1;
        assert_eq!(
            page2[0], expected_page2_first,
            "second page after PageDown should immediately follow the first page"
        );

        // Scenario 2: from an interior offset (start=3), PageDown then PageUp should round-trip.
        let interior_offset = 3usize;
        overlay.view.scroll_offset = interior_offset;
        let before = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_add(page_height);
        let _ = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_sub(page_height);
        let after = transcript_line_numbers(&mut overlay, area);
        assert_eq!(
            before, after,
            "PageDown+PageUp from interior offset ({interior_offset}) should round-trip"
        );

        // Scenario 3: from the top of the second page, PageUp then PageDown should round-trip.
        overlay.view.scroll_offset = page_height;
        let before2 = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_sub(page_height);
        let _ = transcript_line_numbers(&mut overlay, area);
        overlay.view.scroll_offset = overlay.view.scroll_offset.saturating_add(page_height);
        let after2 = transcript_line_numbers(&mut overlay, area);
        assert_eq!(
            before2, after2,
            "PageUp+PageDown from the top of the second page should round-trip"
        );
    }

    #[test]
    fn static_overlay_wraps_long_lines() {
        let mut overlay = static_overlay(
            vec!["a very long line that should wrap when rendered within a narrow pager overlay width".into()],
            "S T A T I C",
        );
        let mut term = Terminal::new(TestBackend::new(24, 8)).expect("term");
        term.draw(|f| overlay.render(f.area(), f.buffer_mut()))
            .expect("draw");
        assert_snapshot!(term.backend());
    }

    #[test]
    fn pager_view_content_height_counts_renderables() {
        let pv = pager_view(
            vec![
                paragraph_block("a", /*lines*/ 2),
                paragraph_block("b", /*lines*/ 3),
            ],
            "T",
            /*scroll_offset*/ 0,
        );

        assert_eq!(pv.content_height(/*width*/ 80), 5);
    }

    #[test]
    fn pager_view_ensure_chunk_visible_scrolls_down_when_needed() {
        let mut pv = pager_view(
            vec![
                paragraph_block("a", /*lines*/ 1),
                paragraph_block("b", /*lines*/ 3),
                paragraph_block("c", /*lines*/ 3),
            ],
            "T",
            /*scroll_offset*/ 0,
        );
        let area = Rect::new(0, 0, 20, 8);

        pv.scroll_offset = 0;
        let content_area = pv.content_area(area);
        pv.ensure_chunk_visible(/*idx*/ 2, content_area);

        let mut buf = Buffer::empty(area);
        pv.render(area, &mut buf);
        let rendered = buffer_to_text(&buf, area);

        assert!(
            rendered.contains("c0"),
            "expected chunk top in view: {rendered:?}"
        );
        assert!(
            rendered.contains("c1"),
            "expected chunk middle in view: {rendered:?}"
        );
        assert!(
            rendered.contains("c2"),
            "expected chunk bottom in view: {rendered:?}"
        );
    }

    #[test]
    fn pager_view_ensure_chunk_visible_scrolls_up_when_needed() {
        let mut pv = pager_view(
            vec![
                paragraph_block("a", /*lines*/ 2),
                paragraph_block("b", /*lines*/ 3),
                paragraph_block("c", /*lines*/ 3),
            ],
            "T",
            /*scroll_offset*/ 0,
        );
        let area = Rect::new(0, 0, 20, 3);

        pv.scroll_offset = 6;
        pv.ensure_chunk_visible(/*idx*/ 0, area);

        assert_eq!(pv.scroll_offset, 0);
    }

    #[test]
    fn pager_view_is_scrolled_to_bottom_accounts_for_wrapped_height() {
        let mut pv = pager_view(
            vec![paragraph_block("a", /*lines*/ 10)],
            "T",
            /*scroll_offset*/ 0,
        );
        let area = Rect::new(0, 0, 20, 8);
        let mut buf = Buffer::empty(area);

        pv.render(area, &mut buf);

        assert!(
            !pv.is_scrolled_to_bottom(),
            "expected view to report not at bottom when offset < max"
        );

        pv.scroll_offset = usize::MAX;
        pv.render(area, &mut buf);

        assert!(
            pv.is_scrolled_to_bottom(),
            "expected view to report at bottom after scrolling to end"
        );
    }
}
