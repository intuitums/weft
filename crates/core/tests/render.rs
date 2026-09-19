use std::io::{self, Write};
use wove::{
    elements::{RichText, Text},
    testing::Screen,
    text::{Span, Wrap},
    Buffer, Canvas, Color, Depth, Element, Rect, Renderer, Style,
};

fn drawn(renderer: &mut Renderer, frame: &Buffer) -> String {
    let mut output = Vec::new();
    renderer.draw(&mut output, frame).unwrap();
    String::from_utf8(output).unwrap()
}

#[test]
fn unchanged_frames_emit_nothing_and_failed_output_forces_a_full_repaint() {
    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("disconnected"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut renderer = Renderer::default();
    let mut frame = Buffer::new(8, 1);
    frame.write(frame.area(), "hello", Style::default());
    assert!(!drawn(&mut renderer, &frame).is_empty());
    assert!(drawn(&mut renderer, &frame).is_empty());
    frame.write(Rect::new(0, 0, 1, 1), "H", Style::default());
    assert!(renderer.draw(&mut Broken, &frame).is_err());
    assert!(drawn(&mut renderer, &frame).contains("Hello"));
}

#[test]
fn a_frame_is_one_synchronized_update_with_every_attribute_and_link() {
    let mut screen = Screen::new(12, 1);
    let style = Style {
        dim: true,
        strikethrough: true,
        ..Style::default()
    };
    let spans = vec![Span::link("docs", style, "https://example.com/a;b")];
    screen
        .tree
        .add(screen.tree.root(), RichText::new(spans, Wrap::None))
        .unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(
        frame.cell(0, 0).unwrap().link(),
        Some("https://example.com/a;b")
    );
    let output = drawn(&mut Renderer::default(), frame);
    assert!(output.starts_with("\x1b[?2026h") && output.ends_with("\x1b[?2026l"));
    assert!(output.contains("\x1b[0;2;9mdocs"));
    let open = output.find("\x1b]8;id=").unwrap();
    let target = output.find(";https://example.com/a;b\x1b\\").unwrap();
    let text = output.find("docs").unwrap();
    let close = output.rfind("\x1b]8;;\x1b\\").unwrap();
    assert!(open < target && target < text && text < close);
}

#[test]
fn a_link_that_could_end_its_escape_sequence_is_drawn_as_plain_text() {
    let mut screen = Screen::new(8, 1);
    let spans = vec![Span::link("x", Style::default(), "https://a\x1b\\evil")];
    screen
        .tree
        .add(screen.tree.root(), RichText::new(spans, Wrap::None))
        .unwrap();
    let output = drawn(&mut Renderer::default(), screen.frame().unwrap());
    assert!(!output.contains("evil"));
}

#[test]
fn colors_map_to_the_nearest_one_the_terminal_accepts() {
    let mut frame = Buffer::new(1, 1);
    let red = Style {
        fg: Color::Rgb(255, 0, 0),
        bold: true,
        ..Style::default()
    };
    frame.write(frame.area(), "x", red);
    let at = |depth| drawn(&mut Renderer::with_depth(depth), &frame);
    assert!(at(Depth::Rgb).contains("\x1b[0;1;38;2;255;0;0mx"));
    assert!(at(Depth::Indexed).contains("\x1b[0;1;38;5;196mx"));
    assert!(at(Depth::Basic).contains("\x1b[0;1;91mx"));
    assert!(at(Depth::Mono).contains("\x1b[0;1mx"));
    let env = |vars: &[(&str, &str)]| {
        Depth::from_env(move |name| {
            let found = vars.iter().find(|(key, _)| *key == name);
            found.map(|(_, value)| value.to_string())
        })
    };
    assert_eq!(
        env(&[("NO_COLOR", "1"), ("COLORTERM", "truecolor")]),
        Depth::Mono
    );
    assert_eq!(env(&[("TERM", "xterm-256color")]), Depth::Indexed);
    // A terminal known for 24-bit color keeps it when COLORTERM was stripped.
    assert_eq!(env(&[("TERM", "xterm-ghostty")]), Depth::Rgb);
    assert_eq!(env(&[("TERM", "linux")]), Depth::Basic);
}

#[test]
fn text_keeps_the_background_beneath_it_until_a_style_resets_it() {
    struct Fill;
    impl Element for Fill {
        fn layout(&self) -> wove::Layout {
            wove::Layout {
                flex_direction: wove::layout::FlexDirection::Column,
                ..Default::default()
            }
        }
        fn paint(&self, canvas: &mut Canvas<'_>) {
            canvas.fill(Style {
                bg: Color::Indexed(4),
                ..Style::default()
            });
        }
    }
    let mut screen = Screen::new(4, 2);
    let fill = screen.tree.add(screen.tree.root(), Fill).unwrap();
    screen.tree.add(fill, Text::new("ab")).unwrap();
    let covering = Style {
        bg: Color::Reset,
        ..Style::default()
    };
    let text = Text {
        style: covering,
        ..Text::new("cd")
    };
    screen.tree.add(fill, text).unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(frame.cell(0, 0).unwrap().style().bg, Color::Indexed(4));
    assert_eq!(frame.cell(0, 1).unwrap().style().bg, Color::Default);
    assert_eq!(frame.cell(3, 1).unwrap().style().bg, Color::Indexed(4));
}

#[test]
fn the_cursor_is_placed_again_after_a_cluster_terminals_measure_differently() {
    let mut frame = Buffer::new(8, 1);
    frame.write(frame.area(), "ab👍cd", Style::default());
    let output = drawn(&mut Renderer::default(), &frame);
    // ASCII runs on; after the emoji the next cell's column is stated outright,
    // so a terminal that thinks the emoji is one cell wide cannot drift.
    assert!(output.contains("ab👍\x1b[1;5Hc"), "{output:?}");
}

#[test]
fn the_cursor_shape_is_sent_again_after_the_session_was_left() {
    let mut screen = Screen::new(8, 1);
    let input = wove::elements::Input {
        cursor: wove::CursorShape::Bar,
        ..Default::default()
    };
    let id = screen.tree.add(screen.tree.root(), input).unwrap();
    screen.tree.focus(Some(id)).unwrap();
    let mut renderer = Renderer::default();
    assert!(drawn(&mut renderer, screen.frame().unwrap()).contains("\x1b[6 q"));
    // Leaving a session resets the shape on the terminal's side.
    renderer.invalidate();
    assert!(drawn(&mut renderer, screen.frame().unwrap()).contains("\x1b[6 q"));
}

#[test]
fn tabs_expand_to_stops_instead_of_vanishing() {
    let mut screen = Screen::new(12, 2);
    screen
        .tree
        .add(screen.tree.root(), Text::new("\tx\nab\ty"))
        .unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["    x       ", "ab  y       "]
    );
}

#[test]
fn a_frame_the_renderer_already_drew_is_skipped_until_the_tree_changes() {
    let mut screen = Screen::new(8, 1);
    let text = screen
        .tree
        .add(screen.tree.root(), Text::new("one"))
        .unwrap();
    let mut renderer = Renderer::default();
    assert!(drawn(&mut renderer, screen.frame().unwrap()).contains("one"));
    assert!(drawn(&mut renderer, screen.frame().unwrap()).is_empty());
    screen
        .tree
        .update::<Text>(text, |text| text.content = "two".into())
        .unwrap();
    assert!(drawn(&mut renderer, screen.frame().unwrap()).contains("two"));
    // A long grapheme and a link live in per-frame tables and still compare equal.
    let mut a = Buffer::new(4, 1);
    let mut b = Buffer::new(4, 1);
    for buffer in [&mut a, &mut b] {
        buffer.write(buffer.area(), "👨‍👩‍👧‍👦", Style::default());
    }
    assert_eq!(a, b);
    assert_eq!(a.cell(0, 0).unwrap().symbol(), "👨‍👩‍👧‍👦");
}
