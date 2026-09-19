//! Multiline editing with a cursor-following viewport and optional soft wrapping.
use crate::{
    text::{clean, edit, Editor},
    Canvas, Element, Event, Response, Style,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// A multiline editor. The viewport follows the cursor. With `wrap`, lines
/// break at words to fit the width and `Up` and `Down` move through the
/// wrapped rows; without it, long lines scroll horizontally.
#[derive(Default)]
pub struct Textarea {
    pub editor: Editor,
    pub style: Style,
    /// Drawn over `style` for atoms, the editor's indivisible tokens.
    pub atom: Style,
    pub placeholder: String,
    pub wrap: bool,
    pub cursor: crate::CursorShape,
}
impl Textarea {
    pub fn new(value: &str) -> Self {
        Self {
            editor: Editor::new(clean(value, true)),
            ..Self::default()
        }
    }
    fn width(&self, width: u16) -> Option<u16> {
        self.wrap.then_some(width.max(1))
    }
}
impl Element for Textarea {
    fn focusable(&self) -> bool {
        true
    }
    fn measure(&self, width: Option<u16>) -> (u16, u16) {
        let (widest, rows) = self.editor.extent_at(width.and_then(|w| self.width(w)));
        (
            widest.clamp(1, usize::from(width.unwrap_or(u16::MAX)).max(1)) as u16,
            rows.min(u16::MAX as usize) as u16,
        )
    }
    fn viewport(&mut self, size: (u16, u16), _: (u32, u32)) -> (u32, u32) {
        self.editor.width = self.width(size.0);
        (0, 0)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        let (width, height) = canvas.size();
        if width == 0 || height == 0 {
            return;
        }
        let value = self.editor.text();
        if value.is_empty() && !canvas.focused() {
            canvas.text(0, 0, &self.placeholder, self.style);
            return;
        }
        let rows = self.editor.rows_at(self.width(width));
        let cursor = self.editor.cursor();
        let row = Editor::row_of(&rows, cursor);
        let col = value[rows[row].start..cursor].width();
        let top = row.saturating_sub(usize::from(height) - 1);
        let left = if self.wrap {
            0
        } else {
            col.saturating_sub(usize::from(width) - 1)
        };
        self.editor.set_view(top, left);
        let selection = self.editor.selection();
        let focused = canvas.focused();
        let selected = |at: usize| focused && selection.contains(&at);
        for (y, range) in rows.iter().enumerate().skip(top).take(usize::from(height)) {
            let mut x = 0;
            for (i, g) in value[range.clone()].grapheme_indices(true) {
                let at = range.start + i;
                let atom = self.editor.atoms().iter().any(|a| a.range.contains(&at));
                let base = if atom { self.atom } else { self.style };
                let style = Style {
                    reverse: base.reverse || selected(at),
                    ..base
                };
                canvas.text(x as i32 - left as i32, (y - top) as i32, g, style);
                x += g.width();
            }
            // A selected line break shows as one reversed cell.
            if value[range.end..].starts_with('\n') && selected(range.end) {
                let style = Style {
                    reverse: true,
                    ..self.style
                };
                canvas.text(x as i32 - left as i32, (y - top) as i32, " ", style);
            }
        }
        if canvas.focused() {
            // Whitespace hung past a wrap seam leaves the cursor on the last cell.
            let x = (col - left).min(usize::from(width) - 1);
            canvas.cursor(x as i32, (row - top) as i32);
            canvas.cursor_shape(self.cursor);
        }
    }
    fn event(&mut self, event: &Event) -> Response {
        edit(&mut self.editor, event, true)
    }
}
