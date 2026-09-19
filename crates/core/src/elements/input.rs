//! Single-line editing with selection and a horizontally scrolling cursor.
use crate::{
    text::{command, Command, Editor},
    Canvas, CursorShape, Element, Event, Layout, Response, Style,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Default)]
pub struct Input {
    pub editor: Editor,
    pub style: Style,
    pub placeholder: String,
    /// Shown in place of every grapheme, for secrets. Clicks are ignored while
    /// set, because cell positions no longer say anything about the text.
    pub mask: Option<char>,
    /// The most graphemes the input accepts; typing and pasting stop there.
    pub limit: Option<usize>,
    pub cursor: CursorShape,
}

impl Input {
    pub fn new(value: &str) -> Self {
        Self {
            editor: Editor::new(
                value
                    .chars()
                    .filter(|c| !c.is_control())
                    .collect::<String>(),
            ),
            ..Self::default()
        }
    }
    /// The cells a grapheme takes as displayed.
    fn cells(&self, grapheme: &str) -> usize {
        self.mask
            .map_or(grapheme.width(), |mask| mask.width().unwrap_or(1))
    }
}

impl Element for Input {
    fn focusable(&self) -> bool {
        true
    }
    fn layout(&self) -> Layout {
        Layout {
            size: taffy::Size {
                width: taffy::prelude::auto(),
                height: taffy::Dimension::length(1.0),
            },
            flex_shrink: 0.0,
            ..Layout::default()
        }
    }
    fn measure(&self, _: Option<u16>) -> (u16, u16) {
        // One cell more than the text, for the cursor after its last grapheme.
        // Without it an input sized to its content scrolls its first cell away.
        let text: usize = self
            .editor
            .text()
            .graphemes(true)
            .map(|g| self.cells(g))
            .sum();
        ((text + 1).min(u16::MAX as usize) as u16, 1)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        let width = usize::from(canvas.size().0);
        if width == 0 {
            return;
        }
        let value = self.editor.text();
        if value.is_empty() && !canvas.focused() {
            canvas.text(0, 0, &self.placeholder, self.style);
            return;
        }
        let cursor_col: usize = value[..self.editor.cursor()]
            .graphemes(true)
            .map(|g| self.cells(g))
            .sum();
        let desired = cursor_col.saturating_sub(width - 1);
        let mut start_col = 0;
        let mut start = 0;
        for (i, g) in value.grapheme_indices(true) {
            if start_col >= desired {
                break;
            }
            start_col += self.cells(g);
            start = i + g.len();
        }
        self.editor.set_view(0, start_col);
        let selection = self.editor.selection();
        let mut mask = [0; 4];
        let mut x = 0;
        for (i, g) in value[start..].grapheme_indices(true) {
            let style = Style {
                reverse: self.style.reverse
                    || (canvas.focused() && selection.contains(&(start + i))),
                ..self.style
            };
            let shown = self.mask.map_or(g, |c| &*c.encode_utf8(&mut mask));
            canvas.text(x, 0, shown, style);
            x += self.cells(g) as i32;
        }
        if canvas.focused() {
            canvas.cursor(cursor_col.saturating_sub(start_col) as i32, 0);
            canvas.cursor_shape(self.cursor);
        }
    }
    fn event(&mut self, event: &Event) -> Response {
        if self.mask.is_some() && matches!(event, Event::Mouse(_)) {
            return Response::IGNORE;
        }
        if let (Some(limit), Some(Command::Insert(text))) = (self.limit, command(event, false)) {
            let editor = &mut self.editor;
            let count = |text: &str| text.graphemes(true).count();
            let kept = count(editor.text()) - count(&editor.text()[editor.selection()]);
            let room: String = text
                .graphemes(true)
                .take(limit.saturating_sub(kept))
                .collect();
            let changed = !room.is_empty() || !editor.selection().is_empty();
            if changed {
                editor.insert(&room);
            }
            return Response {
                handled: true,
                changed,
            };
        }
        crate::text::edit(&mut self.editor, event, false)
    }
}
