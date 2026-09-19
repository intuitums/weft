//! A virtualized column of styled text blocks, anchored at its tail.
use crate::{
    text::{Span, TextLayout, Wrap},
    Canvas, Element, Event, Key, Layout, MouseKind, Response,
};
use std::cell::RefCell;

struct Block {
    spans: Vec<Span>,
    wrap: Wrap,
    /// The layout at the width it was last needed for.
    layout: RefCell<Option<(u16, TextLayout)>>,
}

/// A long document of text blocks, such as a log or a conversation. Only the
/// blocks in view are ever laid out, so a frame costs what is visible however
/// many blocks there are, and appending to the last block relays out only it.
///
/// The view follows the tail until the user scrolls away from it, and stays
/// put while new blocks arrive beneath. Positions are a block and a row within
/// it rather than a row count, because the rows above the view are never
/// measured. Use a `Scroll` of elements instead when the content is not text.
pub struct Feed {
    blocks: Vec<Block>,
    /// Blank rows between blocks.
    gap: u16,
    /// The first visible row as a block and a row within it. `None` follows the tail.
    top: Option<(usize, usize)>,
    /// The size of the last painted viewport, which scrolling is measured against.
    view: (u16, u16),
}

impl Feed {
    pub fn new(gap: u16) -> Self {
        Self {
            blocks: Vec::new(),
            gap,
            top: None,
            view: (0, 0),
        }
    }
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
    /// Append a block and return its index.
    pub fn push(&mut self, spans: Vec<Span>, wrap: Wrap) -> usize {
        self.blocks.push(Block {
            spans,
            wrap,
            layout: RefCell::new(None),
        });
        self.blocks.len() - 1
    }
    /// Replace a block's text, for content that is still arriving.
    pub fn set(&mut self, index: usize, spans: Vec<Span>) {
        if let Some(block) = self.blocks.get_mut(index) {
            block.spans = spans;
            block.layout = RefCell::new(None);
        }
    }
    /// Drop every block from `len` on.
    pub fn truncate(&mut self, len: usize) {
        self.blocks.truncate(len);
        if self.top.is_some_and(|(block, _)| block >= len) {
            self.top = None;
        }
    }
    pub fn following(&self) -> bool {
        self.top.is_none()
    }
    /// Return to the tail and follow it.
    pub fn follow(&mut self) {
        self.top = None;
    }

    /// Rows a block takes at a width, with the gap that separates it from the next.
    fn extent(&self, index: usize, width: u16) -> usize {
        let block = &self.blocks[index];
        let mut slot = block.layout.borrow_mut();
        let rows = match &*slot {
            Some((cached, layout)) if *cached == width => layout.rows(),
            _ => {
                let layout = TextLayout::new(&block.spans, Some(width), block.wrap);
                slot.insert((width, layout)).1.rows()
            }
        };
        rows + usize::from(self.gap) * usize::from(index + 1 < self.blocks.len())
    }

    /// Where a full viewport ending at the tail starts.
    fn tail(&self, (width, height): (u16, u16)) -> (usize, usize) {
        let mut rows = 0;
        for index in (0..self.blocks.len()).rev() {
            rows += self.extent(index, width);
            if rows >= usize::from(height) {
                return (index, rows - usize::from(height));
            }
        }
        (0, 0)
    }

    /// The first visible position: the anchor, unless the tail already fits below it.
    fn start(&self, view: (u16, u16)) -> (usize, usize) {
        let tail = self.tail(view);
        self.top.map_or(tail, |(block, row)| {
            // A block laid out again at a new width may have fewer rows than
            // the anchor remembers; hold its last row rather than point past it.
            let last = self.extent(block, view.0).saturating_sub(1);
            (block, row.min(last)).min(tail)
        })
    }

    fn scroll(&mut self, rows: isize) {
        let (mut block, mut row) = self.start(self.view);
        let width = self.view.0;
        if rows < 0 {
            let mut left = rows.unsigned_abs();
            while left > row && block > 0 {
                left -= row + 1;
                block -= 1;
                row = self.extent(block, width).saturating_sub(1);
            }
            row = row.saturating_sub(left);
        } else {
            // Moving down never passes the tail, so stop at the last block.
            row += rows.unsigned_abs();
            while block + 1 < self.blocks.len() && row >= self.extent(block, width) {
                row -= self.extent(block, width);
                block += 1;
            }
        }
        self.anchor((block, row));
    }

    /// Hold the view at a position, or follow the tail once it is reached.
    fn anchor(&mut self, top: (usize, usize)) {
        self.top = Some(top).filter(|top| *top < self.tail(self.view));
    }
}

impl Element for Feed {
    fn focusable(&self) -> bool {
        true
    }
    fn layout(&self) -> Layout {
        Layout {
            overflow: taffy::Point {
                x: taffy::Overflow::Hidden,
                y: taffy::Overflow::Hidden,
            },
            flex_grow: 1.0,
            ..Layout::default()
        }
    }
    fn viewport(&mut self, size: (u16, u16), _: (u32, u32)) -> (u32, u32) {
        self.view = size;
        (0, 0)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        let view = canvas.size();
        let (first, skipped) = self.start(view);
        let mut y = -(skipped as i32);
        for index in first..self.blocks.len() {
            if y >= i32::from(view.1) {
                break;
            }
            let extent = self.extent(index, view.0);
            if let Some((_, layout)) = &*self.blocks[index].layout.borrow() {
                layout.paint_at(canvas, y);
            }
            y += extent as i32;
        }
    }
    fn event(&mut self, event: &Event) -> Response {
        // Scrolling is measured against a painted viewport.
        if self.view.1 == 0 {
            return Response::IGNORE;
        }
        let page = isize::try_from(self.view.1.max(1)).unwrap_or(1);
        let before = self.start(self.view);
        match event {
            Event::Key(Key::Up, _) => self.scroll(-1),
            Event::Key(Key::Down, _) => self.scroll(1),
            Event::Key(Key::PageUp, _) => self.scroll(-page),
            Event::Key(Key::PageDown, _) => self.scroll(page),
            Event::Key(Key::Home, _) => self.anchor((0, 0)),
            Event::Key(Key::End, _) => self.top = None,
            Event::Mouse(mouse) if mouse.kind == MouseKind::ScrollUp => self.scroll(-1),
            Event::Mouse(mouse) if mouse.kind == MouseKind::ScrollDown => self.scroll(1),
            _ => return Response::IGNORE,
        }
        let moved = self.start(self.view) != before;
        Response {
            handled: moved,
            changed: moved,
        }
    }
}
