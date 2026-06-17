//! Terminal UI helper functions — pure rendering utilities
//! that don't depend on application state.

use ratatui::layout::Rect;

/// Compute a centred popup area of the requested size.
pub fn popup_area(parent: Rect, w: u16, h: u16) -> Rect {
    let x = parent.x + (parent.width.saturating_sub(w) / 2);
    let y = parent.y + (parent.height.saturating_sub(h) / 2);
    Rect::new(x, y, w.min(parent.width), h.min(parent.height))
}
