//! Plain and styled text use the same grapheme layout for measurement and paint.
use crate::{
    text::{Cache, Span, TextLayout, Wrap},
    Canvas, Element, Style,
};

/// The wrapped layout is kept between frames and rebuilt when a field changes.
#[derive(Default)]
pub struct Text {
    pub content: String,
    pub style: Style,
    pub wrap: bool,
    /// The wrapped layout, reused until the fields above change. It is a
    /// field because a struct with a private one cannot be built with
    /// `..Default::default()`; nothing but this element reads it.
    #[doc(hidden)]
    pub cache: Cache,
}
impl Text {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ..Self::default()
        }
    }
    fn rows<T>(&self, width: Option<u16>, read: impl FnOnce(&TextLayout) -> T) -> T {
        let wrap = if self.wrap {
            Wrap::Character
        } else {
            Wrap::None
        };
        let parts = std::iter::once((self.content.as_str(), self.style, None));
        self.cache.with(parts, width, wrap, read)
    }
}
impl Element for Text {
    fn measure(&self, width: Option<u16>) -> (u16, u16) {
        self.rows(width, TextLayout::size)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        self.rows(Some(canvas.size().0), |rows| rows.paint(canvas));
    }
}

/// Styled runs with optional word wrapping. Each span supplies a complete style.
/// The wrapped layout is kept between frames and rebuilt when a field changes.
#[derive(Default)]
pub struct RichText {
    pub spans: Vec<Span>,
    pub wrap: Wrap,
    /// The wrapped layout, reused until the fields above change. It is a
    /// field because a struct with a private one cannot be built with
    /// `..Default::default()`; nothing but this element reads it.
    #[doc(hidden)]
    pub cache: Cache,
}
impl RichText {
    pub fn new(spans: Vec<Span>, wrap: Wrap) -> Self {
        Self {
            spans,
            wrap,
            cache: Cache::default(),
        }
    }
    fn rows<T>(&self, width: Option<u16>, read: impl FnOnce(&TextLayout) -> T) -> T {
        let parts = self
            .spans
            .iter()
            .map(|span| (span.text.as_str(), span.style, span.link.as_ref()));
        self.cache.with(parts, width, self.wrap, read)
    }
}
impl Element for RichText {
    fn measure(&self, width: Option<u16>) -> (u16, u16) {
        self.rows(width, TextLayout::size)
    }
    fn paint(&self, canvas: &mut Canvas<'_>) {
        self.rows(Some(canvas.size().0), |rows| rows.paint(canvas));
    }
}
