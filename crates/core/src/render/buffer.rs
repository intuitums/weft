//! A clipped cell grid that keeps grapheme clusters and wide-cell ownership intact.
use crate::Rect;
use std::{ops::Range, sync::Arc};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Columns between tab stops. Tabs expand to spaces relative to the start of
/// the text being written; they are never sent to the terminal.
pub const TAB: usize = 4;

/// Terminal colors, independent of the terminal backend.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Color {
    /// No color chosen. A foreground uses the terminal default. A background
    /// keeps the color already painted beneath, so text drawn over a filled
    /// parent does not punch a hole in it.
    #[default]
    Default,
    /// Force the terminal default, covering any color painted beneath.
    Reset,
    /// An index in the terminal's 256-color palette.
    Indexed(u8),
    /// A true-color value.
    Rgb(u8, u8, u8),
}

/// Complete cell styling; applications choose their own palette.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
    pub reverse: bool,
}

/// How the terminal draws its cursor while an element shows one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorShape {
    /// Whatever the user configured.
    #[default]
    Default,
    Block,
    Underline,
    Bar,
}
impl CursorShape {
    /// The DECSCUSR parameter for the steady form of the shape.
    pub(crate) fn code(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Block => 2,
            Self::Underline => 4,
            Self::Bar => 6,
        }
    }
}

/// Bytes of a grapheme a slot holds inline. Longer clusters, such as emoji
/// families, go to the buffer's table.
const INLINE: usize = 15;
/// The length that marks a grapheme kept in the buffer's table.
const LONG: u8 = u8::MAX;

/// One stored cell: small, flat, and `Copy`, so that clearing, comparing, and
/// copying frames are plain memory operations. Anything that needs a pointer,
/// a long grapheme or a hyperlink, is an index into a table on the buffer.
/// Colors are packed into integers so that equality is a few word compares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Slot {
    /// The grapheme and, in the last byte, its length. A long grapheme keeps
    /// its table index in the first four bytes.
    glyph: [u8; INLINE + 1],
    fg: u32,
    bg: u32,
    /// Attribute bits: `BOLD`, `DIM`, and the rest.
    flags: u8,
    /// Zero marks the continuation of a wide grapheme.
    pub(crate) width: u8,
    /// One more than an index into the buffer's links; zero is no link.
    link: u16,
}

impl Slot {
    const BLANK: Self = {
        let mut glyph = [0; INLINE + 1];
        glyph[0] = b' ';
        glyph[INLINE] = 1;
        Self {
            glyph,
            fg: 0,
            bg: 0,
            flags: 0,
            width: 1,
            link: 0,
        }
    };
    fn len(&self) -> u8 {
        self.glyph[INLINE]
    }
    /// A blank cell in the default style with no link.
    pub(crate) fn is_blank(&self) -> bool {
        *self == Self::BLANK
    }
    /// One ASCII byte, whose width every terminal agrees on.
    pub(crate) fn is_ascii(&self) -> bool {
        self.len() == 1 && self.glyph[0] < 0x80
    }
    /// Whether comparing this slot with one from another buffer needs the tables.
    fn indirect(&self) -> bool {
        self.link != 0 || self.len() == LONG
    }
    pub(crate) fn style(&self) -> Style {
        let flag = |bit: u8| self.flags & bit != 0;
        Style {
            fg: unpack(self.fg),
            bg: unpack(self.bg),
            bold: flag(BOLD),
            dim: flag(DIM),
            italic: flag(ITALIC),
            underline: flag(UNDERLINE),
            strikethrough: flag(STRIKETHROUGH),
            reverse: flag(REVERSE),
        }
    }
}

/// A color as one integer: a tag in the high byte, then the channels.
fn pack(color: Color) -> u32 {
    match color {
        Color::Default => 0,
        Color::Reset => 1 << 24,
        Color::Indexed(n) => 2 << 24 | u32::from(n),
        Color::Rgb(r, g, b) => 3 << 24 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b),
    }
}
fn unpack(color: u32) -> Color {
    let [tag, r, g, b] = color.to_be_bytes();
    match tag {
        1 => Color::Reset,
        2 => Color::Indexed(b),
        3 => Color::Rgb(r, g, b),
        _ => Color::Default,
    }
}

/// A style resolved once for a run of cells. A default background is filled
/// in per cell from whatever is beneath.
pub(crate) struct Brush {
    slot: Slot,
    inherit: bool,
}

/// A slot's attribute bits.
const BOLD: u8 = 1;
const DIM: u8 = 1 << 1;
const ITALIC: u8 = 1 << 2;
const UNDERLINE: u8 = 1 << 3;
const STRIKETHROUGH: u8 = 1 << 4;
const REVERSE: u8 = 1 << 5;

fn flags(style: Style) -> u8 {
    let bit = |on: bool, bit: u8| if on { bit } else { 0 };
    bit(style.bold, BOLD)
        | bit(style.dim, DIM)
        | bit(style.italic, ITALIC)
        | bit(style.underline, UNDERLINE)
        | bit(style.strikethrough, STRIKETHROUGH)
        | bit(style.reverse, REVERSE)
}

/// One terminal cell of a frame. Wide graphemes own following continuation cells.
#[derive(Clone, Copy)]
pub struct Cell<'a> {
    slot: &'a Slot,
    buffer: &'a Buffer,
}

impl<'a> Cell<'a> {
    /// Grapheme text; an empty string denotes a wide-grapheme continuation.
    pub fn symbol(&self) -> &'a str {
        self.buffer.symbol(self.slot)
    }
    /// The resolved style. Backgrounds are never `Reset`; they are the color shown.
    pub fn style(&self) -> Style {
        self.slot.style()
    }
    /// The hyperlink target this cell belongs to.
    pub fn link(&self) -> Option<&'a str> {
        self.buffer.link(self.slot).map(|link| &**link)
    }
}

/// An owned frame. All text writes are clipped and reject terminal controls.
#[derive(Debug)]
pub struct Buffer {
    width: u16,
    height: u16,
    pub(crate) cells: Vec<Slot>,
    /// Graphemes too long for a slot.
    long: Vec<Box<str>>,
    links: Vec<Arc<str>>,
    pub(crate) cursor: Option<(u16, u16)>,
    pub(crate) shape: CursorShape,
    /// Set by a tree to a number unique to each frame it paints, and zeroed
    /// by any write. A renderer that sees a number it already drew skips the
    /// frame without comparing a cell.
    pub(crate) version: u64,
}

impl Clone for Buffer {
    fn clone(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            cells: self.cells.clone(),
            long: self.long.clone(),
            links: self.links.clone(),
            cursor: self.cursor,
            shape: self.shape,
            version: self.version,
        }
    }
    /// Reuses the cell storage, so keeping a copy of each frame does not allocate.
    fn clone_from(&mut self, source: &Self) {
        self.width = source.width;
        self.height = source.height;
        self.cells.clone_from(&source.cells);
        self.long.clone_from(&source.long);
        self.links.clone_from(&source.links);
        self.cursor = source.cursor;
        self.shape = source.shape;
        self.version = source.version;
    }
}

/// Frames are equal when they show the same thing, whatever their tables hold.
impl PartialEq for Buffer {
    fn eq(&self, other: &Self) -> bool {
        self.area() == other.area()
            && self.cursor == other.cursor
            && self.shape == other.shape
            && (0..self.height).all(|y| self.same_row(other, y))
    }
}
impl Eq for Buffer {}

impl Buffer {
    /// Allocate a blank frame of the requested dimensions.
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            cursor: None,
            cells: vec![Slot::BLANK; usize::from(width) * usize::from(height)],
            long: Vec::new(),
            links: Vec::new(),
            shape: CursorShape::Default,
            version: 0,
        }
    }
    pub fn cursor(&self) -> Option<(u16, u16)> {
        self.cursor
    }
    pub fn cursor_shape(&self) -> CursorShape {
        self.shape
    }

    /// The full drawable area.
    pub fn area(&self) -> Rect {
        Rect::new(0, 0, self.width, self.height)
    }
    /// Inspect a cell without permitting broken wide-grapheme ownership.
    pub fn cell(&self, x: u16, y: u16) -> Option<Cell<'_>> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let slot = self
            .cells
            .get(usize::from(y) * usize::from(self.width) + usize::from(x))?;
        Some(Cell { slot, buffer: self })
    }
    /// One row of slots; empty when the row is outside the frame.
    pub(crate) fn row(&self, y: u16) -> &[Slot] {
        if y >= self.height {
            return &[];
        }
        let start = usize::from(y) * usize::from(self.width);
        &self.cells[start..start + usize::from(self.width)]
    }
    pub(crate) fn symbol<'a>(&'a self, slot: &'a Slot) -> &'a str {
        if slot.len() == LONG {
            let [a, b, c, d, ..] = slot.glyph;
            let index = u32::from_le_bytes([a, b, c, d]) as usize;
            return self.long.get(index).map_or("", |text| text);
        }
        // Inline bytes are always copied from a whole `str`.
        std::str::from_utf8(&slot.glyph[..usize::from(slot.len())]).unwrap_or("")
    }
    pub(crate) fn link(&self, slot: &Slot) -> Option<&Arc<str>> {
        self.links.get(usize::from(slot.link).checked_sub(1)?)
    }
    /// Whether row `y` shows the same thing in both frames. Rows are compared
    /// as plain memory unless a cell refers to a table.
    pub(crate) fn same_row(&self, other: &Self, y: u16) -> bool {
        let (a, b) = (self.row(y), other.row(y));
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| self.same_cell(a, other, b))
    }
    pub(crate) fn same_cell(&self, a: &Slot, other: &Self, b: &Slot) -> bool {
        if !a.indirect() && !b.indirect() {
            return a == b;
        }
        a.width == b.width
            && a.style() == b.style()
            && self.symbol(a) == other.symbol(b)
            && self.link(a) == other.link(b)
    }
    /// Restore every cell to its blank default.
    pub fn clear(&mut self) {
        self.cells.fill(Slot::BLANK);
        self.long.clear();
        self.links.clear();
        self.cursor = None;
        self.shape = CursorShape::Default;
        self.version = 0;
    }
    /// The rows from `first` on as a frame of their own.
    pub(crate) fn tail(&self, first: u16) -> Self {
        let first = first.min(self.height);
        Self {
            width: self.width,
            height: self.height - first,
            cells: self.cells[usize::from(first) * usize::from(self.width)..].to_vec(),
            long: self.long.clone(),
            links: self.links.clone(),
            cursor: self
                .cursor
                .and_then(|(x, y)| Some((x, y.checked_sub(first)?))),
            shape: self.shape,
            version: 0,
        }
    }

    /// Erase the complete grapheme covering an index, including its trailing
    /// cells. Erased cells keep their background, as the fill beneath them would.
    fn erase(&mut self, index: usize) {
        let row = index / usize::from(self.width) * usize::from(self.width);
        let mut start = index;
        while start > row && self.cells[start].width == 0 {
            start -= 1;
        }
        let end = (start + usize::from(self.cells[start].width)).min(row + usize::from(self.width));
        for cell in &mut self.cells[start..end] {
            *cell = Slot {
                bg: cell.bg,
                ..Slot::BLANK
            };
        }
    }

    /// Place one grapheme of a known width. The caller has already clipped it,
    /// rejected controls, and checked that it fits the row.
    pub(crate) fn put(
        &mut self,
        x: u16,
        y: u16,
        grapheme: &str,
        width: usize,
        style: Style,
        link: Option<&Arc<str>>,
    ) {
        self.version = 0;
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        // Only a wide grapheme, or one beneath, leaves cells to clean up.
        if width != 1 || self.cells[index].width != 1 {
            for i in index..index + width {
                self.erase(i);
            }
        }
        let brush = self.brush(style, link);
        let mut glyph = [0; INLINE + 1];
        if grapheme.len() <= INLINE {
            glyph[..grapheme.len()].copy_from_slice(grapheme.as_bytes());
            glyph[INLINE] = grapheme.len() as u8;
        } else {
            glyph[..4].copy_from_slice(&(self.long.len() as u32).to_le_bytes());
            glyph[INLINE] = LONG;
            self.long.push(grapheme.into());
        }
        let slot = Slot {
            glyph,
            bg: if brush.inherit {
                self.cells[index].bg
            } else {
                brush.slot.bg
            },
            width: width as u8,
            ..brush.slot
        };
        self.cells[index] = slot;
        for cell in &mut self.cells[index + 1..index + width] {
            *cell = Slot {
                glyph: [0; INLINE + 1],
                width: 0,
                ..slot
            };
        }
    }

    /// Resolve a style and link once, for stamping a run of ASCII cells.
    pub(crate) fn brush(&mut self, style: Style, link: Option<&Arc<str>>) -> Brush {
        let color = |color| match color {
            Color::Reset => 0,
            color => pack(color),
        };
        Brush {
            inherit: style.bg == Color::Default,
            slot: Slot {
                fg: color(style.fg),
                bg: color(style.bg),
                flags: flags(style),
                link: link.map_or(0, |link| self.intern(link)),
                ..Slot::BLANK
            },
        }
    }

    /// Place one printable ASCII byte. This is the hot path of painting text:
    /// no segmentation, no width lookup, and no per-cell style work.
    pub(crate) fn stamp(&mut self, x: u16, y: u16, byte: u8, brush: &Brush) {
        self.version = 0;
        let index = usize::from(y) * usize::from(self.width) + usize::from(x);
        if self.cells[index].width != 1 {
            self.erase(index);
        }
        let mut slot = brush.slot;
        slot.glyph[0] = byte;
        if brush.inherit {
            slot.bg = self.cells[index].bg;
        }
        self.cells[index] = slot;
    }

    /// A link's table index plus one. Runs of cells share their link, so only
    /// the newest entry is checked; a full table drops further links.
    fn intern(&mut self, link: &Arc<str>) -> u16 {
        if !self.links.last().is_some_and(|last| last == link) {
            if self.links.len() >= usize::from(u16::MAX) {
                return 0;
            }
            self.links.push(link.clone());
        }
        self.links.len() as u16
    }

    /// Write one line within `area`, returning columns used. Tabs expand to the
    /// next stop; other control characters and standalone zero-width graphemes
    /// are skipped. A grapheme never splits. Overwriting either half of a wide
    /// grapheme clears the whole old grapheme.
    pub fn write(&mut self, area: Rect, text: &str, style: Style) -> u16 {
        let area = area.intersection(self.area());
        if area.height == 0 || area.width == 0 {
            return 0;
        }
        let mut used = 0usize;
        for (grapheme, width) in graphemes(text) {
            let (grapheme, width, count) = if grapheme == "\t" {
                (" ", 1, TAB - used % TAB)
            } else {
                (grapheme, width, 1)
            };
            for _ in 0..count {
                if used + width > usize::from(area.width) {
                    return used as u16;
                }
                self.put(area.x + used as u16, area.y, grapheme, width, style, None);
                used += width;
            }
        }
        used as u16
    }

    /// The cells between two positions inclusive, in reading order, as a row
    /// and a column range within it. Either end may come first and either may
    /// lie outside the frame, as a selection does after a resize; the result
    /// is clamped and may be empty.
    fn span(&self, a: (u16, u16), b: (u16, u16)) -> impl Iterator<Item = (u16, Range<usize>)> {
        let (from, to) = if (a.1, a.0) <= (b.1, b.0) {
            (a, b)
        } else {
            (b, a)
        };
        let width = usize::from(self.width);
        let rows = if width == 0 { 0 } else { self.height };
        (from.1..rows.min(to.1.saturating_add(1))).map(move |y| {
            let start = if y == from.1 { usize::from(from.0) } else { 0 };
            let end = if y == to.1 {
                usize::from(to.0) + 1
            } else {
                width
            };
            (y, start.min(width)..end.min(width).max(start.min(width)))
        })
    }

    /// Swap foreground and background over a run of cells in reading order,
    /// the way a selection is shown.
    pub(crate) fn invert(&mut self, from: (u16, u16), to: (u16, u16)) {
        let width = usize::from(self.width);
        let rows: Vec<_> = self.span(from, to).collect();
        for (y, columns) in rows {
            let row = usize::from(y) * width;
            for slot in &mut self.cells[row + columns.start..row + columns.end] {
                slot.flags ^= REVERSE;
            }
        }
        self.version = 0;
    }

    /// The text of a run of cells in reading order, one line per row with
    /// trailing blanks removed, as a user would expect to copy it.
    pub fn text(&self, from: (u16, u16), to: (u16, u16)) -> String {
        let lines: Vec<String> = self
            .span(from, to)
            .map(|(y, columns)| {
                let row = &self.row(y)[columns];
                let line: String = row.iter().map(|slot| self.symbol(slot)).collect();
                line.trim_end().to_owned()
            })
            .collect();
        lines.join("\n")
    }

    /// Get visible rows as plain text, useful for snapshots and diagnostics.
    pub fn lines(&self) -> Vec<String> {
        (0..self.height)
            .map(|y| self.row(y).iter().map(|slot| self.symbol(slot)).collect())
            .collect()
    }
}

/// Grapheme clusters with their byte offsets. ASCII text, the common case in a
/// terminal, is one cluster per byte and skips Unicode segmentation entirely.
/// The one difference is that an ASCII `\r\n` arrives as two clusters; both
/// are controls, which every caller drops or splits on.
pub(crate) enum Clusters<'a> {
    Ascii(&'a str, usize),
    Unicode(unicode_segmentation::GraphemeIndices<'a>),
}
pub(crate) fn clusters(text: &str) -> Clusters<'_> {
    if text.is_ascii() {
        Clusters::Ascii(text, 0)
    } else {
        Clusters::Unicode(text.grapheme_indices(true))
    }
}
impl<'a> Iterator for Clusters<'a> {
    type Item = (usize, &'a str);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Ascii(text, at) => {
                let cluster = text.get(*at..*at + 1)?;
                *at += 1;
                Some((*at - 1, cluster))
            }
            Self::Unicode(clusters) => clusters.next(),
        }
    }
}

/// Cells a cluster occupies. Controls, tabs included, and zero-width clusters
/// report none; callers that expand tabs check for them first.
pub(crate) fn cluster_width(cluster: &str) -> usize {
    match cluster.as_bytes() {
        [byte] => usize::from((0x20..0x7f).contains(byte)),
        _ if cluster.chars().any(char::is_control) => 0,
        _ => cluster.width(),
    }
}

/// Printable graphemes with their cell widths. Tabs are yielded with a zero
/// width for the caller to expand; other controls and zero-width clusters are
/// dropped, as are clusters too wide for a cell to record.
pub(crate) fn graphemes(text: &str) -> impl Iterator<Item = (&str, usize)> {
    clusters(text).filter_map(|(_, g)| {
        if g == "\t" {
            return Some((g, 0));
        }
        let width = cluster_width(g);
        (width > 0 && width <= usize::from(u8::MAX)).then_some((g, width))
    })
}
