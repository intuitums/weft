//! A fixed-height list that asks its row provider only for visible rows.
use super::window;
use crate::{text::Span, Button, Canvas, Element, Event, Key, MouseKind, Response, Style};
use unicode_width::UnicodeWidthStr;

/// Rows have one terminal line each and carry their own styles. The application
/// owns the underlying data; `count` and `row` allow lists larger than terminal
/// coordinate limits. The selected row is drawn entirely in `highlight`.
pub struct List {
    pub count: usize,
    pub selected: usize,
    pub width: u16,
    pub highlight: Style,
    row: Box<dyn Fn(usize) -> Vec<Span>>,
    page: usize,
    /// The first row shown. It moves only when the selection leaves the view.
    offset: usize,
}
impl List {
    pub fn new(count: usize, width: u16, row: impl Fn(usize) -> Vec<Span> + 'static) -> Self {
        Self {
            count,
            selected: 0,
            width,
            highlight: Style {
                reverse: true,
                ..Default::default()
            },
            row: Box::new(row),
            page: 1,
            offset: 0,
        }
    }
}
impl Element for List {
    fn layout(&self) -> crate::Layout {
        crate::Layout {
            overflow: taffy::Point {
                x: taffy::Overflow::Hidden,
                y: taffy::Overflow::Hidden,
            },
            flex_grow: 1.0,
            ..crate::Layout::default()
        }
    }
    fn focusable(&self) -> bool {
        self.count > 0
    }
    fn measure(&self, _: Option<u16>) -> (u16, u16) {
        (self.width, self.count.min(u16::MAX as usize) as u16)
    }
    fn viewport(&mut self, size: (u16, u16), _: (u32, u32)) -> (u32, u32) {
        self.page = usize::from(size.1).max(1);
        self.offset = window(self.offset, self.selected, self.count, self.page);
        (0, 0)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        let height = usize::from(canvas.size().1);
        let selected = self.selected.min(self.count.saturating_sub(1));
        let offset = window(self.offset, selected, self.count, height);
        for (y, index) in (offset..self.count).take(height).enumerate() {
            let mut x = 0;
            for span in (self.row)(index) {
                let style = if index == selected {
                    self.highlight
                } else {
                    span.style
                };
                match &span.link {
                    Some(url) => canvas.link(x, y as i32, &span.text, style, url),
                    None => canvas.text(x, y as i32, &span.text, style),
                }
                x += span.text.width() as i32;
            }
        }
    }
    fn event(&mut self, event: &Event) -> Response {
        let old = self.selected;
        let first = window(self.offset, old, self.count, self.page);
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                MouseKind::Down(Button::Left) => self.selected = first + usize::from(mouse.y),
                MouseKind::ScrollUp => self.selected = self.selected.saturating_sub(1),
                MouseKind::ScrollDown => self.selected = self.selected.saturating_add(1),
                _ => return Response::IGNORE,
            },
            Event::Key(Key::Up, _) => self.selected = self.selected.saturating_sub(1),
            Event::Key(Key::Down, _) => self.selected = self.selected.saturating_add(1),
            Event::Key(Key::PageUp, _) => self.selected = self.selected.saturating_sub(self.page),
            Event::Key(Key::PageDown, _) => self.selected = self.selected.saturating_add(self.page),
            Event::Key(Key::Home, _) => self.selected = 0,
            Event::Key(Key::End, _) => self.selected = self.count.saturating_sub(1),
            _ => return Response::IGNORE,
        }
        self.selected = self.selected.min(self.count.saturating_sub(1));
        self.offset = window(first, self.selected, self.count, self.page);
        Response {
            handled: true,
            changed: self.selected != old,
        }
    }
}
