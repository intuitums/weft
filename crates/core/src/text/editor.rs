//! An owned text editor shared by input elements and custom elements.
use super::{wrap, Wrap};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    ops::Range,
    rc::Rc,
};
use unicode_segmentation::{GraphemeCursor, UnicodeSegmentation};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct State {
    text: String,
    cursor: usize,
    anchor: Option<usize>,
}

/// A run of text that behaves as one unit: the cursor steps over it, and an
/// edit that touches it removes all of it. Applications use atoms for tokens
/// that stand for something else, such as a collapsed paste or an attachment,
/// and recognize them by `id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Atom {
    pub id: u64,
    pub range: Range<usize>,
}

/// Where a cursor movement or a deletion ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motion {
    Left,
    Right,
    WordLeft,
    WordRight,
    /// The start and end of the logical line.
    LineHome,
    LineEnd,
    /// The start and end of the text.
    Home,
    End,
    /// One display row, keeping the preferred column.
    Up,
    Down,
}

/// An edit as data, so applications can bind keys to commands of their choice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Insert(String),
    /// Move the cursor, extending the selection when the flag is set.
    Move(Motion, bool),
    /// Delete the selection, or else from the cursor to where the motion ends.
    Delete(Motion),
    SelectAll,
    Undo,
    Redo,
}

/// An edit stores only the replaced text, the selection endpoints, and the
/// atoms as they were before it.
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
    before: (usize, Option<usize>),
    after: (usize, Option<usize>),
    atoms: Vec<Atom>,
}

/// Positions are byte offsets on grapheme boundaries. Undo retains at most 100
/// edits; a run of typed characters within one word is a single edit.
#[derive(Default)]
pub struct Editor {
    state: State,
    atoms: Vec<Atom>,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    column: Option<usize>,
    /// The last edit was typed text that the next typed grapheme may join.
    typing: bool,
    /// Display width for soft wrapping. When set, `Up` and `Down` move through
    /// wrapped rows instead of logical lines. A `Textarea` sets it to its
    /// painted width on every frame; set it yourself only in a custom element.
    pub width: Option<u16>,
    /// The display row and column at the top-left of the element showing this
    /// editor, recorded while painting so a click can be mapped to a position.
    view: Cell<(usize, usize)>,
    /// Wrapped rows, until the text changes. Layout asks at a few widths per
    /// pass, so a few are kept.
    rows: RefCell<Vec<Rows>>,
}

/// The display rows of the text at one wrapping width.
struct Rows {
    width: Option<u16>,
    ranges: Rc<Vec<Range<usize>>>,
    /// The width of the widest row.
    widest: usize,
}

impl Editor {
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            state: State {
                cursor: text.len(),
                text,
                anchor: None,
            },
            ..Self::default()
        }
    }
    pub fn text(&self) -> &str {
        &self.state.text
    }
    pub fn cursor(&self) -> usize {
        self.state.cursor
    }
    pub fn selection(&self) -> Range<usize> {
        let a = self.state.anchor.unwrap_or(self.state.cursor);
        a.min(self.state.cursor)..a.max(self.state.cursor)
    }
    pub fn atoms(&self) -> &[Atom] {
        &self.atoms
    }
    /// Replace the text, keeping the display width and dropping history and atoms.
    pub fn set(&mut self, text: impl Into<String>) {
        *self = Self {
            width: self.width,
            ..Self::new(text)
        };
    }

    pub fn apply(&mut self, command: Command) {
        match command {
            Command::Insert(text) => self.insert(&text),
            Command::Move(motion, extend) => self.travel(motion, extend),
            Command::Delete(motion) => self.erase(motion),
            Command::SelectAll => self.select_all(),
            Command::Undo => self.undo(),
            Command::Redo => self.redo(),
        }
    }

    /// Insert at the cursor, replacing the selection as one undoable operation.
    pub fn insert(&mut self, text: &str) {
        if text.is_empty() && self.selection().is_empty() {
            return;
        }
        let mut range = self.selection();
        for atom in &self.atoms {
            if atom.range.start < range.end && atom.range.end > range.start {
                range = range.start.min(atom.range.start)..range.end.max(atom.range.end);
            }
        }
        let before = (self.state.cursor, self.state.anchor);
        let atoms = self.atoms.clone();
        let removed = self.state.text[range.clone()].to_owned();
        self.state.text.replace_range(range.clone(), text);
        self.atoms
            .retain(|atom| atom.range.end <= range.start || atom.range.start >= range.end);
        for atom in &mut self.atoms {
            if atom.range.start >= range.end {
                atom.range.start = atom.range.start - range.len() + text.len();
                atom.range.end = atom.range.end - range.len() + text.len();
            }
        }
        self.rows.borrow_mut().clear();
        let end = range.start + text.len();
        // Insertion can join adjacent graphemes; advance to the next valid boundary.
        // The check looks only at the text around `end`, whatever the length.
        let mut at = GraphemeCursor::new(end, self.state.text.len(), true);
        self.state.cursor = match at.is_boundary(&self.state.text, 0) {
            Ok(true) => end,
            _ => at
                .next_boundary(&self.state.text, 0)
                .ok()
                .flatten()
                .unwrap_or(self.state.text.len()),
        };
        self.state.anchor = None;
        self.column = None;
        self.redo.clear();
        let typed = removed.is_empty() && text != "\n" && text.graphemes(true).count() == 1;
        let after = (self.state.cursor, None);
        match self.undo.back_mut() {
            // A word and the whitespace after it undo together; the next word does not.
            Some(last)
                if self.typing
                    && typed
                    && last.start + last.inserted.len() == range.start
                    && (!last.inserted.ends_with(char::is_whitespace)
                        || text.chars().all(char::is_whitespace)) =>
            {
                last.inserted.push_str(text);
                last.after = after;
            }
            _ => {
                if self.undo.len() == 100 {
                    self.undo.pop_front();
                }
                self.undo.push_back(Edit {
                    start: range.start,
                    removed,
                    inserted: text.to_owned(),
                    before,
                    after,
                    atoms,
                });
            }
        }
        self.typing = typed;
    }

    /// Insert text that stays one unit until an edit removes it.
    pub fn insert_atom(&mut self, text: &str, id: u64) {
        if text.is_empty() {
            return;
        }
        self.typing = false;
        self.insert(text);
        self.typing = false;
        let end = self.state.cursor;
        let at = self.atoms.partition_point(|atom| atom.range.start < end);
        self.atoms.insert(
            at,
            Atom {
                id,
                range: end - text.len()..end,
            },
        );
    }

    /// Where a motion from the cursor ends, before atoms are stepped over.
    fn target(&mut self, motion: Motion) -> usize {
        let (text, cursor) = (&self.state.text, self.state.cursor);
        match motion {
            Motion::Left => text[..cursor]
                .grapheme_indices(true)
                .next_back()
                .map_or(0, |(i, _)| i),
            Motion::Right => cursor + text[cursor..].graphemes(true).next().map_or(0, str::len),
            Motion::WordLeft => text[..cursor]
                .split_word_bound_indices()
                .rev()
                .find(|(_, word)| !word.chars().all(char::is_whitespace))
                .map_or(0, |(i, _)| i),
            Motion::WordRight => text[cursor..]
                .split_word_bound_indices()
                .find(|(_, word)| !word.chars().all(char::is_whitespace))
                .map_or(text.len(), |(i, word)| cursor + i + word.len()),
            Motion::LineHome => text[..cursor].rfind('\n').map_or(0, |i| i + 1),
            Motion::LineEnd => text[cursor..].find('\n').map_or(text.len(), |i| cursor + i),
            Motion::Home => 0,
            Motion::End => text.len(),
            Motion::Up => self.row_target(-1),
            Motion::Down => self.row_target(1),
        }
    }

    /// Display rows as byte ranges: logical lines, soft-wrapped at `width`.
    /// A row's range excludes the newline that ends its line.
    pub fn rows(&self) -> Rc<Vec<Range<usize>>> {
        self.rows_at(self.width)
    }

    /// The widest row and the number of rows at a width.
    pub fn extent_at(&self, width: Option<u16>) -> (usize, usize) {
        let rows = self.rows_at(width).len();
        let cached = self.rows.borrow();
        let widest = cached.iter().find(|rows| rows.width == width);
        (widest.map_or(0, |rows| rows.widest), rows)
    }

    /// `rows` at an explicit width, for measuring before a width is assigned.
    /// Rows are wrapped once per edit and width, not once per paint or cursor
    /// move, which is what keeps a large document responsive.
    pub fn rows_at(&self, width: Option<u16>) -> Rc<Vec<Range<usize>>> {
        if let Some(rows) = self.rows.borrow().iter().find(|rows| rows.width == width) {
            return rows.ranges.clone();
        }
        let mut rows = Vec::new();
        let mut base = 0;
        for line in self.state.text.split('\n') {
            match width {
                Some(width) => rows.extend(
                    wrap(line, usize::from(width), Wrap::Word)
                        .into_iter()
                        .map(|row| base + row.start..base + row.end),
                ),
                None => rows.push(base..base + line.len()),
            }
            base += line.len() + 1;
        }
        let widest = rows
            .iter()
            .map(|row: &Range<usize>| self.state.text[row.clone()].width())
            .max()
            .unwrap_or(0);
        let rows = Rc::new(rows);
        let mut cached = self.rows.borrow_mut();
        if cached.len() == 3 {
            cached.remove(0);
        }
        cached.push(Rows {
            width,
            ranges: rows.clone(),
            widest,
        });
        rows
    }

    pub(crate) fn set_view(&self, top: usize, left: usize) {
        self.view.set((top, left));
    }

    /// The position shown at a cell of the element displaying this editor:
    /// the nearest grapheme boundary at or before it, on the nearest row.
    pub fn position_at(&self, x: u16, y: u16) -> usize {
        let (top, left) = self.view.get();
        let rows = self.rows();
        let index = (top + usize::from(y)).min(rows.len() - 1);
        self.offset_in(&rows, index, left + usize::from(x))
    }

    /// The display row holding a position. A position on a wrap seam belongs
    /// to the row it starts.
    pub fn row_of(rows: &[Range<usize>], position: usize) -> usize {
        rows.partition_point(|row| row.start <= position)
            .saturating_sub(1)
    }

    fn row_target(&mut self, delta: isize) -> usize {
        let rows = self.rows();
        let row = Self::row_of(&rows, self.state.cursor);
        let column = self
            .column
            .unwrap_or_else(|| self.state.text[rows[row].start..self.state.cursor].width());
        self.column = Some(column);
        match row.checked_add_signed(delta).filter(|i| *i < rows.len()) {
            Some(index) => self.offset_in(&rows, index, column),
            None => self.state.cursor,
        }
    }

    /// The position at a display column of a row: the last grapheme boundary
    /// at or before it. The end of a soft-wrapped row is the start of the next
    /// one, so the last position that still belongs to such a row is before
    /// its end.
    fn offset_in(&self, rows: &[Range<usize>], index: usize, column: usize) -> usize {
        let row = &rows[index];
        let soft = rows
            .get(index + 1)
            .is_some_and(|after| after.start == row.end);
        let (mut width, mut offset) = (0, row.start);
        for g in self.state.text[row.clone()].graphemes(true) {
            if width + g.width() > column || soft && offset + g.len() >= row.end {
                break;
            }
            width += g.width();
            offset += g.len();
        }
        offset
    }

    /// The nearest grapheme boundary at or before an offset.
    fn boundary(&self, offset: usize) -> usize {
        let text = self.text();
        if offset >= text.len() {
            return text.len();
        }
        let mut offset = offset;
        while !text.is_char_boundary(offset) {
            offset -= 1;
        }
        let mut at = GraphemeCursor::new(offset, text.len(), true);
        match at.is_boundary(text, 0) {
            Ok(true) => offset,
            _ => at.prev_boundary(text, 0).ok().flatten().unwrap_or(0),
        }
    }

    fn move_to(&mut self, target: usize, extend: bool) {
        let target = self.boundary(target);
        // Step over an atom in the direction of travel.
        let target = self
            .atoms
            .iter()
            .find(|atom| atom.range.start < target && target < atom.range.end)
            .map_or(target, |atom| {
                if target < self.state.cursor {
                    atom.range.start
                } else {
                    atom.range.end
                }
            });
        self.typing = false;
        if extend {
            self.state.anchor.get_or_insert(self.state.cursor);
        } else {
            self.state.anchor = None;
        }
        self.state.cursor = target;
    }

    /// Move the cursor. Without `extend`, a horizontal step out of a selection
    /// lands on the selection's edge.
    pub fn travel(&mut self, motion: Motion, extend: bool) {
        let selection = self.selection();
        let target = match motion {
            Motion::Left if !extend && !selection.is_empty() => selection.start,
            Motion::Right if !extend && !selection.is_empty() => selection.end,
            _ => self.target(motion),
        };
        let column = self
            .column
            .filter(|_| matches!(motion, Motion::Up | Motion::Down));
        self.move_to(target, extend);
        self.column = column;
    }

    /// Delete the selection, or else from the cursor to where the motion ends.
    /// Undo restores the cursor and selection as they were before the deletion.
    pub fn erase(&mut self, motion: Motion) {
        let before = (self.state.cursor, self.state.anchor);
        if self.selection().is_empty() {
            let target = self.target(motion);
            self.move_to(target, true);
        }
        if self.selection().is_empty() {
            (self.state.cursor, self.state.anchor) = before;
            return;
        }
        self.insert("");
        if let Some(edit) = self.undo.back_mut() {
            edit.before = before;
        }
    }

    pub fn left(&mut self, extend: bool) {
        self.travel(Motion::Left, extend);
    }
    pub fn right(&mut self, extend: bool) {
        self.travel(Motion::Right, extend);
    }
    pub fn home(&mut self, extend: bool) {
        self.travel(Motion::Home, extend);
    }
    pub fn end(&mut self, extend: bool) {
        self.travel(Motion::End, extend);
    }
    /// Move to the beginning of the current logical line.
    pub fn line_home(&mut self, extend: bool) {
        self.travel(Motion::LineHome, extend);
    }
    /// Move to the end of the current logical line.
    pub fn line_end(&mut self, extend: bool) {
        self.travel(Motion::LineEnd, extend);
    }
    /// Move by display rows while preserving the preferred display column.
    pub fn vertical(&mut self, delta: isize, extend: bool) {
        for _ in 0..delta.unsigned_abs() {
            self.travel(if delta < 0 { Motion::Up } else { Motion::Down }, extend);
        }
    }
    pub fn select_all(&mut self) {
        self.column = None;
        self.typing = false;
        self.state.anchor = Some(0);
        self.state.cursor = self.text().len();
    }
    pub fn backspace(&mut self) {
        self.erase(Motion::Left);
    }
    pub fn delete(&mut self) {
        self.erase(Motion::Right);
    }
    /// Move to a byte offset, snapping backward to a grapheme boundary.
    pub fn seek(&mut self, offset: usize, extend: bool) {
        self.column = None;
        self.move_to(offset, extend);
    }
    pub fn undo(&mut self) {
        if let Some(mut edit) = self.undo.pop_back() {
            self.state
                .text
                .replace_range(edit.start..edit.start + edit.inserted.len(), &edit.removed);
            (self.state.cursor, self.state.anchor) = edit.before;
            self.rows.borrow_mut().clear();
            std::mem::swap(&mut self.atoms, &mut edit.atoms);
            self.column = None;
            self.typing = false;
            self.redo.push(edit);
        }
    }
    pub fn redo(&mut self) {
        if let Some(mut edit) = self.redo.pop() {
            self.state
                .text
                .replace_range(edit.start..edit.start + edit.removed.len(), &edit.inserted);
            (self.state.cursor, self.state.anchor) = edit.after;
            self.rows.borrow_mut().clear();
            std::mem::swap(&mut self.atoms, &mut edit.atoms);
            self.column = None;
            self.typing = false;
            self.undo.push_back(edit);
        }
    }
}
