//! A table with explicit column widths and a fixed header.
use super::window;
use crate::{Button, Canvas, Element, Event, Key, MouseKind, Response, Style};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Column widths are terminal cells. Values are clipped at grapheme boundaries;
/// headers remain visible while keyboard navigation scrolls the body.
#[derive(Default)]
pub struct Table {
    pub columns: Vec<(String, u16)>,
    pub rows: Vec<Vec<String>>,
    pub selected: usize,
    pub style: Style,
    pub header: Style,
    pub highlight: Style,
    page: usize,
    /// The first body row shown. It moves only when the selection leaves the view.
    offset: usize,
}
impl Table {
    pub fn new(columns: Vec<(String, u16)>, rows: Vec<Vec<String>>) -> Self {
        Self {
            columns,
            rows,
            header: Style {
                bold: true,
                ..Default::default()
            },
            highlight: Style {
                reverse: true,
                ..Default::default()
            },
            ..Self::default()
        }
    }
    fn row(
        &self,
        canvas: &mut Canvas<'_>,
        y: i32,
        values: impl Iterator<Item = String>,
        style: Style,
    ) {
        let mut x = 0;
        for ((_, width), value) in self.columns.iter().zip(values) {
            let mut used = 0;
            let clipped: String = value
                .graphemes(true)
                .take_while(|g| {
                    used += g.width();
                    used <= usize::from(*width)
                })
                .collect();
            canvas.text(x, y, &clipped, style);
            x += i32::from(*width) + 1;
        }
    }
}
impl Element for Table {
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
        !self.rows.is_empty()
    }
    fn measure(&self, _: Option<u16>) -> (u16, u16) {
        (
            self.columns
                .iter()
                .fold(0u16, |n, (_, w)| n.saturating_add(*w).saturating_add(1))
                .saturating_sub(1),
            self.rows.len().saturating_add(1).min(u16::MAX as usize) as u16,
        )
    }
    fn viewport(&mut self, size: (u16, u16), _: (u32, u32)) -> (u32, u32) {
        self.page = usize::from(size.1.saturating_sub(1)).max(1);
        self.offset = window(self.offset, self.selected, self.rows.len(), self.page);
        (0, 0)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        self.row(
            canvas,
            0,
            self.columns.iter().map(|(name, _)| name.clone()),
            self.header,
        );
        let height = usize::from(canvas.size().1.saturating_sub(1));
        let selected = self.selected.min(self.rows.len().saturating_sub(1));
        let offset = window(self.offset, selected, self.rows.len(), height);
        for (y, (index, row)) in self
            .rows
            .iter()
            .enumerate()
            .skip(offset)
            .take(height)
            .enumerate()
        {
            self.row(
                canvas,
                y as i32 + 1,
                row.iter().cloned(),
                if index == selected {
                    self.highlight
                } else {
                    self.style
                },
            );
        }
    }
    fn event(&mut self, event: &Event) -> Response {
        let old = self.selected;
        let first = window(self.offset, old, self.rows.len(), self.page);
        match event {
            Event::Mouse(mouse) => match mouse.kind {
                // Row zero is the header.
                MouseKind::Down(Button::Left) if mouse.y > 0 => {
                    self.selected = first + usize::from(mouse.y) - 1
                }
                MouseKind::ScrollUp => self.selected = self.selected.saturating_sub(1),
                MouseKind::ScrollDown => self.selected = self.selected.saturating_add(1),
                _ => return Response::IGNORE,
            },
            Event::Key(Key::Up, _) => self.selected = self.selected.saturating_sub(1),
            Event::Key(Key::Down, _) => self.selected = self.selected.saturating_add(1),
            Event::Key(Key::PageUp, _) => self.selected = self.selected.saturating_sub(self.page),
            Event::Key(Key::PageDown, _) => self.selected = self.selected.saturating_add(self.page),
            Event::Key(Key::Home, _) => self.selected = 0,
            Event::Key(Key::End, _) => self.selected = self.rows.len().saturating_sub(1),
            _ => return Response::IGNORE,
        }
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
        self.offset = window(first, self.selected, self.rows.len(), self.page);
        Response {
            handled: true,
            changed: self.selected != old,
        }
    }
}
