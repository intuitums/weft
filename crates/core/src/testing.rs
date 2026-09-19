//! The same element tree and input dispatch used by an interactive terminal.
use crate::{Buffer, Dispatch, Error, Event, Tree};

pub struct Screen {
    pub tree: Tree,
    width: u16,
    height: u16,
}
impl Screen {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            tree: Tree::new(),
            width,
            height,
        }
    }
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }
    pub fn frame(&mut self) -> Result<&Buffer, Error> {
        self.tree.frame(self.width, self.height)
    }
    pub fn send(&mut self, event: impl Into<Event>) -> Result<Dispatch, Error> {
        self.frame()?;
        self.tree.dispatch(event.into())
    }
    /// Press and release the left button on a cell.
    pub fn click(&mut self, x: u16, y: u16) -> Result<Dispatch, Error> {
        use crate::{Button, Mouse, MouseKind};
        let press = self.send(Event::Mouse(Mouse::new(
            x,
            y,
            MouseKind::Down(Button::Left),
        )))?;
        self.send(Event::Mouse(Mouse::new(x, y, MouseKind::Up(Button::Left))))?;
        Ok(press)
    }
    /// Press on one cell, drag to another, and release there.
    pub fn drag(&mut self, from: (u16, u16), to: (u16, u16)) -> Result<Dispatch, Error> {
        use crate::{Button, Mouse, MouseKind};
        let at = |(x, y), kind| Event::Mouse(Mouse::new(x, y, kind));
        self.send(at(from, MouseKind::Down(Button::Left)))?;
        let dragged = self.send(at(to, MouseKind::Drag(Button::Left)))?;
        self.send(at(to, MouseKind::Up(Button::Left)))?;
        Ok(dragged)
    }
}

/// A clock advanced explicitly by tests, with no sleeping or wall-clock reads.
#[derive(Default)]
pub struct Clock {
    now: std::time::Duration,
}
impl Clock {
    pub fn now(&self) -> std::time::Duration {
        self.now
    }
    pub fn advance(&mut self, elapsed: std::time::Duration) {
        self.now = self.now.saturating_add(elapsed);
    }
}

/// Retains only changed frames, including their styles and cursor positions.
#[derive(Default)]
pub struct Recorder {
    frames: Vec<(std::time::Duration, Buffer)>,
}
impl Recorder {
    pub fn record(&mut self, at: std::time::Duration, frame: &Buffer) {
        if self
            .frames
            .last()
            .is_none_or(|(_, previous)| previous != frame)
        {
            self.frames.push((at, frame.clone()));
        }
    }
    pub fn frames(&self) -> &[(std::time::Duration, Buffer)] {
        &self.frames
    }
    pub fn clear(&mut self) {
        self.frames.clear();
    }
}
