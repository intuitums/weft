//! Element drawing in local coordinates, clipped by the tree's viewport.
use super::buffer::{graphemes, TAB};
use crate::{Buffer, CursorShape, Rect, Style};
use std::{ops::Range, sync::Arc};

/// The line style of a border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Border {
    #[default]
    Single,
    Rounded,
    Double,
    Heavy,
}

/// An element cannot reach cells outside its assigned clip through this interface.
pub struct Canvas<'a> {
    pub(crate) buffer: &'a mut Buffer,
    pub(crate) origin: (i32, i32),
    pub(crate) clip: Rect,
    pub(crate) size: (u16, u16),
    pub(crate) focused: bool,
}

impl Canvas<'_> {
    pub fn size(&self) -> (u16, u16) {
        self.size
    }
    pub fn focused(&self) -> bool {
        self.focused
    }
    /// Local rows that survive clipping. Elements with many rows paint only these.
    pub fn visible_rows(&self) -> Range<i32> {
        let top = i32::from(self.clip.y) - self.origin.1;
        top..top + i32::from(self.clip.height)
    }

    /// Write a single line; partially clipped graphemes are omitted as a whole.
    /// Tabs expand to stops measured from the start of `text`.
    pub fn text(&mut self, x: i32, y: i32, text: &str, style: Style) {
        self.run(x, y, text, style, None);
    }

    /// Write a single line whose cells open `url` in terminals that support
    /// hyperlinks. Runs sharing a target highlight together, across rows too.
    pub fn link(&mut self, x: i32, y: i32, text: &str, style: Style, url: &Arc<str>) {
        self.run(x, y, text, style, Some(url));
    }

    fn run(&mut self, x: i32, y: i32, text: &str, style: Style, link: Option<&Arc<str>>) {
        let y = self.origin.1.saturating_add(y);
        if y < i32::from(self.clip.y) || y >= i32::from(self.clip.y) + i32::from(self.clip.height) {
            return;
        }
        let start = self.origin.0.saturating_add(x);
        let mut x = start;
        let right = i32::from(self.clip.x) + i32::from(self.clip.width);
        if text.is_ascii() {
            // One cell per byte: the style is resolved once and each byte stamped.
            let brush = self.buffer.brush(style, link);
            for &byte in text.as_bytes() {
                let (byte, count) = match byte {
                    b'\t' => (b' ', TAB - (x - start) as usize % TAB),
                    0x20..0x7f => (byte, 1),
                    _ => continue,
                };
                for _ in 0..count {
                    if x >= right {
                        return;
                    }
                    if x >= i32::from(self.clip.x) {
                        self.buffer.stamp(x as u16, y as u16, byte, &brush);
                    }
                    x += 1;
                }
            }
            return;
        }
        for (grapheme, width) in graphemes(text) {
            let (grapheme, width, count) = if grapheme == "\t" {
                (" ", 1, TAB - (x - start) as usize % TAB)
            } else {
                (grapheme, width, 1)
            };
            for _ in 0..count {
                if x >= right {
                    return;
                }
                if x >= i32::from(self.clip.x) && x + width as i32 <= right {
                    self.buffer
                        .put(x as u16, y as u16, grapheme, width, style, link);
                }
                x = x.saturating_add(width as i32);
            }
        }
    }

    pub fn cursor(&mut self, x: i32, y: i32) {
        let x = self.origin.0.saturating_add(x);
        let y = self.origin.1.saturating_add(y);
        if x >= 0
            && y >= 0
            && x <= i32::from(u16::MAX)
            && y <= i32::from(u16::MAX)
            && self.clip.contains(x as u16, y as u16)
        {
            self.buffer.cursor = Some((x as u16, y as u16));
            self.buffer.version = 0;
        }
    }

    /// The shape of the cursor this element shows.
    pub fn cursor_shape(&mut self, shape: CursorShape) {
        self.buffer.shape = shape;
        self.buffer.version = 0;
    }

    /// Blank the visible part of the element in `style`. A `Default` background
    /// keeps what is beneath; use `Color::Reset` to cover it.
    pub fn fill(&mut self, style: Style) {
        let brush = self.buffer.brush(style, None);
        for y in self.clip.y..self.clip.y + self.clip.height {
            for x in self.clip.x..self.clip.x + self.clip.width {
                self.buffer.stamp(x, y, b' ', &brush);
            }
        }
    }

    /// Outline the element. Nothing is drawn when it is too small to have an inside.
    pub fn border(&mut self, style: Style, border: Border) {
        let (w, h) = self.size;
        if w < 2 || h < 2 {
            return;
        }
        let [left, right, bottom_left, bottom_right, across, down] = match border {
            Border::Single => ["┌", "┐", "└", "┘", "─", "│"],
            Border::Rounded => ["╭", "╮", "╰", "╯", "─", "│"],
            Border::Double => ["╔", "╗", "╚", "╝", "═", "║"],
            Border::Heavy => ["┏", "┓", "┗", "┛", "━", "┃"],
        };
        let line = across.repeat(usize::from(w - 2));
        self.text(0, 0, &format!("{left}{line}{right}"), style);
        let bottom = format!("{bottom_left}{line}{bottom_right}");
        self.text(0, i32::from(h - 1), &bottom, style);
        for y in 1..h - 1 {
            self.text(0, i32::from(y), down, style);
            self.text(i32::from(w - 1), i32::from(y), down, style);
        }
    }
}
