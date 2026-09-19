use crate::{
    elements::RichText,
    text::{Span, Wrap},
    Color, Style,
};
use syntect::{
    easy::HighlightLines,
    highlighting::{FontStyle, Theme},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};

/// Reuse a syntax set and application-selected theme across documents.
pub struct Highlighter {
    pub syntaxes: SyntaxSet,
    pub theme: Theme,
}
impl Highlighter {
    pub fn new(syntaxes: SyntaxSet, theme: Theme) -> Self {
        Self { syntaxes, theme }
    }
    /// Highlight by syntax token or file extension. Unknown languages stay plain.
    /// Spans carry no background, so the text sits on whatever is beneath it;
    /// fill the container with the theme's background to show it.
    pub fn highlight(&self, source: &str, language: &str) -> Result<RichText, syntect::Error> {
        let syntax = self
            .syntaxes
            .find_syntax_by_token(language)
            .unwrap_or_else(|| self.syntaxes.find_syntax_plain_text());
        let mut highlighter = HighlightLines::new(syntax, &self.theme);
        let mut spans = Vec::new();
        for line in LinesWithEndings::from(source) {
            for (style, text) in highlighter.highlight_line(line, &self.syntaxes)? {
                spans.push(Span::new(
                    text,
                    Style {
                        fg: Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b),
                        bold: style.font_style.contains(FontStyle::BOLD),
                        italic: style.font_style.contains(FontStyle::ITALIC),
                        underline: style.font_style.contains(FontStyle::UNDERLINE),
                        ..Style::default()
                    },
                ));
            }
        }
        Ok(RichText::new(spans, Wrap::None))
    }
}
pub use syntect::{highlighting::ThemeSet, parsing::SyntaxSet as Syntaxes};
