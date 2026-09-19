//! Persistent terminal elements with layout, input routing, and headless rendering.
//!
//! ```
//! use wove::{Tree, elements::Text};
//! let mut tree = Tree::new();
//! let text = tree.add(tree.root(), Text::new("Hello"))?;
//! tree.update::<Text>(text, |w| w.content.push('!'))?;
//! let frame = tree.frame(80, 24)?;
//! assert_eq!(frame.cell(5, 0).unwrap().symbol(), "!");
//! # Ok::<(), wove::Error>(())
//! ```
#![forbid(unsafe_code)]

pub mod animation;
mod element;
pub use element::Element;
pub mod elements;
pub mod input;
pub mod render;
pub mod testing;
pub mod text;
mod tree;

pub use input::{Button, Event, Key, Modifiers, Mouse, MouseKind, Response};
pub use render::{
    Border, Buffer, Canvas, Cell, Color, CursorShape, Depth, Inline, Options, Rect, Renderer,
    ScreenMode, Style,
};
pub use taffy::Style as Layout;
pub use tree::{Dispatch, Error, Id, Tree};
/// Taffy's layout types and helpers, measured in terminal cells.
pub mod layout {
    pub use taffy::prelude::*;
}

#[cfg(feature = "terminal")]
pub mod terminal;

/// Styled line differences. Enable the `diff` feature.
#[cfg(feature = "diff")]
pub mod diff;
/// Markdown formatted as styled text. Enable the `markdown` feature.
#[cfg(feature = "markdown")]
pub mod markdown;
/// Syntax highlighting with application-selected grammars and themes.
#[cfg(feature = "syntax")]
pub mod syntax;
