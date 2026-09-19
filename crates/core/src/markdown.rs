use crate::{
    elements::RichText,
    text::{Span, Wrap},
    Style,
};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};

/// Complete styles for Markdown roles, chosen by the application.
#[derive(Clone, Copy, Default)]
pub struct Palette {
    pub text: Style,
    pub heading: Style,
    pub code: Style,
    pub link: Style,
}

/// Render CommonMark as styled text. HTML remains literal text. Link text opens
/// its destination in terminals with hyperlinks, and the destination is shown
/// for the rest. Block layout is linear and does not implement HTML layout.
pub fn render(source: &str, palette: Palette) -> RichText {
    let mut spans = Vec::new();
    let mut styles = vec![palette.text];
    let mut links = Vec::new();
    let mut lists: Vec<Option<u64>> = Vec::new();
    for event in Parser::new(source) {
        let current = *styles.last().unwrap_or(&palette.text);
        match event {
            Event::Start(tag) => {
                let mut next = current;
                match tag {
                    Tag::Heading { .. } => next = palette.heading,
                    Tag::Strong => next.bold = true,
                    Tag::Emphasis => next.italic = true,
                    Tag::CodeBlock(_) => next = palette.code,
                    Tag::Link { dest_url, .. } => {
                        links.push(dest_url.to_string());
                        next = palette.link;
                    }
                    Tag::List(start) => lists.push(start),
                    Tag::Item => {
                        let prefix = match lists.last_mut() {
                            Some(Some(n)) => {
                                let s = format!("{n}. ");
                                *n += 1;
                                s
                            }
                            _ => "• ".into(),
                        };
                        spans.push(Span::new(prefix, current));
                    }
                    Tag::BlockQuote(_) => spans.push(Span::new("│ ", current)),
                    _ => {}
                }
                styles.push(next);
            }
            Event::End(tag) => {
                styles.pop();
                match tag {
                    TagEnd::Paragraph
                    | TagEnd::Heading(_)
                    | TagEnd::CodeBlock
                    | TagEnd::BlockQuote(_) => spans.push(Span::new("\n\n", palette.text)),
                    TagEnd::Item => spans.push(Span::new("\n", palette.text)),
                    TagEnd::List(_) => {
                        lists.pop();
                    }
                    TagEnd::Link => {
                        if let Some(url) = links.pop() {
                            spans.push(Span::new(format!(" ({url})"), palette.link));
                        }
                    }
                    _ => {}
                }
            }
            Event::Text(text) | Event::Html(text) | Event::InlineHtml(text) => {
                spans.push(match links.last() {
                    Some(url) => Span::link(text.into_string(), current, url.as_str()),
                    None => Span::new(text.into_string(), current),
                })
            }
            Event::Code(text) => spans.push(Span::new(text.into_string(), palette.code)),
            Event::SoftBreak => spans.push(Span::new(" ", current)),
            Event::HardBreak => spans.push(Span::new("\n", current)),
            Event::Rule => spans.push(Span::new("────\n", current)),
            _ => {}
        }
    }
    RichText::new(spans, Wrap::Word)
}
