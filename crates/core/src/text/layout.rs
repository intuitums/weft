//! Shared grapheme layout for plain and styled text.
use crate::{
    render::{cluster_width, clusters, TAB},
    Canvas, Style,
};
use std::{cell::RefCell, ops::Range, sync::Arc};

/// A run of text with one complete terminal style and an optional hyperlink.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub text: String,
    pub style: Style,
    /// Opened by terminals that support hyperlinks; a wrapped link stays one link.
    pub link: Option<Arc<str>>,
}
impl Span {
    pub fn new(text: impl Into<String>, style: Style) -> Self {
        Self {
            text: text.into(),
            style,
            link: None,
        }
    }
    pub fn link(text: impl Into<String>, style: Style, url: impl Into<Arc<str>>) -> Self {
        Self {
            link: Some(url.into()),
            ..Self::new(text, style)
        }
    }
}

/// Wrapping never divides a grapheme. Word wrapping falls back to character wrapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Wrap {
    #[default]
    None,
    Character,
    Word,
}

/// Break one logical line into display rows no wider than `width`, as byte
/// ranges. Word wrapping breaks after whitespace and falls back to breaking
/// inside a word that is wider than a row. There is always at least one row.
pub fn wrap(line: &str, width: usize, wrap: Wrap) -> Vec<Range<usize>> {
    let mut rows = Vec::new();
    let (mut start, mut columns) = (0, 0);
    // Byte offset and column just after the row's last whitespace.
    let mut gap: Option<(usize, usize)> = None;
    for (offset, g) in clusters(line) {
        let space = g != "\t" && g.chars().all(char::is_whitespace);
        if wrap == Wrap::Word && space && columns + cell_width(g, columns) > width.max(1) {
            // Whitespace at the seam hangs past the row instead of opening the next.
            gap = Some((offset + g.len(), columns));
            continue;
        }
        // A word break can leave a tail that still overflows with this grapheme.
        while wrap != Wrap::None
            && columns + cell_width(g, columns) > width.max(1)
            && offset > start
        {
            let (end, used) = gap
                .take()
                .filter(|(end, _)| wrap == Wrap::Word && *end > start)
                .unwrap_or((offset, columns));
            rows.push(start..end);
            start = end;
            columns -= used;
        }
        columns += cell_width(g, columns);
        if g.chars().all(char::is_whitespace) {
            gap = Some((offset + g.len(), columns));
        }
    }
    rows.push(start..line.len());
    rows
}

/// Columns a grapheme occupies when it starts at `column`. Tabs reach the next
/// stop; control characters and zero-width clusters take none and are not drawn.
fn cell_width(g: &str, column: usize) -> usize {
    if g == "\t" {
        TAB - column % TAB
    } else {
        cluster_width(g)
    }
}

struct Run {
    range: Range<usize>,
    span: usize,
    /// Columns from the start of the row.
    x: usize,
    /// Set for a tab, which paints as this many spaces.
    tab: Option<usize>,
}

/// Measured rows used unchanged by painting. Span boundaries cannot split graphemes.
pub struct TextLayout {
    content: String,
    styles: Vec<(Style, Option<Arc<str>>)>,
    rows: Vec<Vec<Run>>,
    width: usize,
}
impl TextLayout {
    pub fn new(spans: &[Span], width: Option<u16>, mode: Wrap) -> Self {
        let content: String = spans.iter().map(|s| s.text.as_str()).collect();
        let limit = width.map_or(usize::MAX, usize::from);
        let mut ends = Vec::new();
        let mut end = 0;
        for span in spans {
            end += span.text.len();
            ends.push(end);
        }
        let mut rows = Vec::new();
        let mut widest = 0;
        let mut base = 0;
        for raw in content.split('\n') {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            for range in wrap(line, limit, mode) {
                let mut row: Vec<Run> = Vec::new();
                let mut x = 0;
                for (offset, g) in clusters(&line[range.clone()]) {
                    let n = cell_width(g, x);
                    // Skip what cannot be drawn, and whitespace hung past the seam.
                    if n == 0 || mode != Wrap::None && x + n > limit.max(1) {
                        continue;
                    }
                    let at = base + range.start + offset;
                    let span = ends.partition_point(|end| *end <= at);
                    let tab = (g == "\t").then_some(n);
                    match row.last_mut() {
                        Some(run)
                            if run.span == span
                                && run.tab.is_none()
                                && tab.is_none()
                                && run.range.end == at =>
                        {
                            run.range.end += g.len()
                        }
                        _ => row.push(Run {
                            range: at..at + g.len(),
                            span,
                            x,
                            tab,
                        }),
                    }
                    x += n;
                }
                widest = widest.max(x);
                rows.push(row);
            }
            base += raw.len() + 1;
        }
        Self {
            styles: spans.iter().map(|s| (s.style, s.link.clone())).collect(),
            content,
            rows,
            width: widest,
        }
    }
    pub fn size(&self) -> (u16, u16) {
        (
            self.width.min(u16::MAX as usize) as u16,
            self.rows.len().min(u16::MAX as usize) as u16,
        )
    }
    /// Whether this layout was built from exactly these runs.
    fn matches<'a>(&self, parts: impl Iterator<Item = Part<'a>>) -> bool {
        let (mut at, mut count) = (0, 0);
        for (text, style, link) in parts {
            let same = self.styles.get(count).is_some_and(|(s, l)| {
                *s == style && l.as_ref() == link && self.content[at..].starts_with(text)
            });
            if !same {
                return false;
            }
            at += text.len();
            count += 1;
        }
        at == self.content.len() && count == self.styles.len()
    }
    /// The number of display rows, which may exceed what `size` can report.
    pub fn rows(&self) -> usize {
        self.rows.len()
    }
    pub fn paint(&self, canvas: &mut Canvas<'_>) {
        self.paint_at(canvas, 0);
    }
    /// Draw with the first row at local row `top`. Only rows that survive
    /// clipping are drawn, so a long text costs what is visible of it.
    pub fn paint_at(&self, canvas: &mut Canvas<'_>, top: i32) {
        let visible = canvas.visible_rows();
        let first = visible.start.saturating_sub(top).max(0) as usize;
        let last = (visible.end.saturating_sub(top).max(0) as usize).min(self.rows.len());
        for (index, row) in self.rows.iter().enumerate().take(last).skip(first) {
            let y = top.saturating_add(index as i32);
            for run in row {
                let (style, link) = self
                    .styles
                    .get(run.span)
                    .map_or((Style::default(), None), |(s, l)| (*s, l.as_ref()));
                let spaces;
                let text = match run.tab {
                    Some(n) => {
                        spaces = " ".repeat(n);
                        spaces.as_str()
                    }
                    None => &self.content[run.range.clone()],
                };
                match link {
                    Some(url) => canvas.link(run.x as i32, y, text, style, url),
                    None => canvas.text(run.x as i32, y, text, style),
                }
            }
        }
    }
}

/// One styled run as an element holds it: text, style, and link.
pub(crate) type Part<'a> = (&'a str, Style, Option<&'a Arc<str>>);

/// The last layout an element computed, reused until its inputs change.
/// Elements keep public fields, so there is no moment of change to hook;
/// instead the inputs are compared with the text the layout already holds,
/// which is exact and costs a memory comparison.
#[derive(Default)]
pub struct Cache(RefCell<Option<(Option<u16>, Wrap, TextLayout)>>);
impl Cache {
    pub(crate) fn with<'a, T>(
        &self,
        parts: impl Iterator<Item = Part<'a>> + Clone,
        width: Option<u16>,
        wrap: Wrap,
        read: impl FnOnce(&TextLayout) -> T,
    ) -> T {
        let mut slot = self.0.borrow_mut();
        match &*slot {
            Some((w, mode, layout))
                if *w == width && *mode == wrap && layout.matches(parts.clone()) =>
            {
                read(layout)
            }
            _ => {
                let spans: Vec<Span> = parts
                    .map(|(text, style, link)| Span {
                        text: text.to_owned(),
                        style,
                        link: link.cloned(),
                    })
                    .collect();
                read(
                    &slot
                        .insert((width, wrap, TextLayout::new(&spans, width, wrap)))
                        .2,
                )
            }
        }
    }
}
