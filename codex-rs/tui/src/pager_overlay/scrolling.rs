//! Viewport-aware transcript rendering and the fallback for generic pager content.

use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;

use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use crate::render::renderable::Renderable;
use crate::style::user_message_style;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::HyperlinkParagraph;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::Widget;

/// Renders a committed history cell directly into the visible transcript viewport.
pub(super) struct CellRenderable {
    pub(super) cell: Arc<dyn HistoryCell>,
    pub(super) cell_index: usize,
    pub(super) highlighted: bool,
    pub(super) workspace: bool,
    pub(super) emphasize_user: bool,
    pub(super) workspace_target: Option<Rc<Cell<Option<usize>>>>,
}

impl Renderable for CellRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_scrolled(area, buf, /*scroll_offset*/ 0);
    }

    /// Scroll visible text and hyperlink metadata together without rendering hidden rows.
    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        let hyperlink_lines = if self.workspace {
            self.cell.workspace_transcript_hyperlink_lines(area.width)
        } else {
            self.cell.transcript_hyperlink_lines(area.width)
        };
        let highlighted = self.highlighted
            || self
                .workspace_target
                .as_ref()
                .is_some_and(|target| target.get() == Some(self.cell_index));
        let hyperlink_lines = if self.workspace && highlighted {
            mark_workspace_target(hyperlink_lines)
        } else {
            hyperlink_lines
        };
        let style = if self.cell.as_any().is::<UserHistoryCell>() {
            if self.emphasize_user || highlighted {
                user_message_style().reversed()
            } else {
                user_message_style()
            }
        } else {
            Style::default()
        };
        HyperlinkParagraph::new(&hyperlink_lines, style)
            .scroll(scroll_offset)
            .render(area, buf);
        true
    }

    fn desired_height(&self, width: u16) -> u16 {
        if self.workspace {
            self.cell.desired_workspace_transcript_height(width)
        } else {
            self.cell.desired_transcript_height(width)
        }
    }
}

/// Replace the standard user-turn marker only for the selected workspace target.
///
/// The replacement glyph has the same terminal width as `›`, so hyperlink column ranges remain
/// valid and no re-wrapping is introduced by the marker.
fn mark_workspace_target(mut lines: Vec<HyperlinkLine>) -> Vec<HyperlinkLine> {
    for line in &mut lines {
        let Some(first_span) = line.line.spans.first_mut() else {
            continue;
        };
        let content = first_span.content.as_ref();
        let Some(rest) = content.strip_prefix("› ") else {
            continue;
        };
        first_span.content = format!("▸ {rest}").into();
        break;
    }
    lines
}

/// Renders the optional in-flight transcript tail without allocating hidden rows.
pub(super) struct HyperlinkLinesRenderable {
    pub(super) lines: Vec<HyperlinkLine>,
}

impl Renderable for HyperlinkLinesRenderable {
    fn render(&self, area: Rect, buf: &mut Buffer) {
        self.render_scrolled(area, buf, /*scroll_offset*/ 0);
    }

    /// Keep live-tail hyperlinks aligned with the same visible rows as their text.
    fn render_scrolled(&self, area: Rect, buf: &mut Buffer, scroll_offset: u16) -> bool {
        HyperlinkParagraph::new(&self.lines, Style::default())
            .scroll(scroll_offset)
            .render(area, buf);
        true
    }

    fn desired_height(&self, width: u16) -> u16 {
        HyperlinkParagraph::new(&self.lines, Style::default())
            .line_count(width)
            .try_into()
            .unwrap_or(/*default*/ 0)
    }
}

/// Render visible rows directly when supported, preserving the legacy scratch-buffer fallback.
pub(super) fn render_offset_content(
    area: Rect,
    buf: &mut Buffer,
    renderable: &dyn Renderable,
    scroll_offset: u16,
) -> u16 {
    let height = renderable.desired_height(area.width);
    let copy_height = area.height.min(height.saturating_sub(scroll_offset));
    if copy_height == 0 {
        return 0;
    }

    let visible_area = Rect::new(area.x, area.y, area.width, copy_height);
    if renderable.render_scrolled(visible_area, buf, scroll_offset) {
        return copy_height;
    }

    let mut tall_buf = Buffer::empty(Rect::new(
        /*x*/ 0,
        /*y*/ 0,
        area.width,
        scroll_offset + copy_height,
    ));
    renderable.render(*tall_buf.area(), &mut tall_buf);
    for y in 0..copy_height {
        let src_y = y + scroll_offset;
        for x in 0..area.width {
            buf[(area.x + x, area.y + y)] = tall_buf[(x, src_y)].clone();
        }
    }

    copy_height
}

#[cfg(test)]
#[path = "scrolling_tests.rs"]
mod tests;
