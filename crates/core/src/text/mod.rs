//! Text storage, editing, and display layout shared by terminal elements.
mod editor;
pub use editor::{Atom, Command, Editor, Motion};
mod input;
pub use input::command;
pub(crate) use input::{clean, edit};
mod layout;
#[doc(hidden)]
pub use layout::Cache;
pub use layout::{wrap, Span, TextLayout, Wrap};
