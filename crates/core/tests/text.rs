use wove::{text::Editor, Buffer, Style};
#[test]
fn selection_replaces_whole_graphemes_and_undo_restores_it() {
    let mut e = Editor::new("a👩‍💻e\u{301}");
    e.left(true);
    e.left(true);
    assert_eq!(&e.text()[e.selection()], "👩‍💻e\u{301}");
    e.insert("界");
    assert_eq!(e.text(), "a界");
    e.undo();
    assert_eq!(&e.text()[e.selection()], "👩‍💻e\u{301}");
    e.redo();
    assert_eq!(e.text(), "a界");
}
#[test]
fn insertion_that_joins_a_cluster_keeps_cursor_on_a_boundary() {
    let mut e = Editor::new("👩💻");
    e.left(false);
    e.insert("\u{200d}");
    assert_eq!(e.cursor(), e.text().len());
    e.backspace();
    assert_eq!(e.text(), "");
}
#[test]
fn overwriting_a_wide_continuation_erases_the_entire_glyph() {
    let mut b = Buffer::new(4, 1);
    let area = b.area();
    b.write(area, "界x", Style::default());
    b.write(wove::Rect::new(1, 0, 3, 1), "a", Style::default());
    assert_eq!(b.cell(0, 0).unwrap().symbol(), " ");
    assert_eq!(b.cell(1, 0).unwrap().symbol(), "a");
    assert_eq!(b.cell(2, 0).unwrap().symbol(), "x");
}

#[test]
fn vertical_movement_preserves_display_column_through_short_lines() {
    let mut editor = Editor::new("界ab\nx\n界cd");
    editor.vertical(-1, false);
    assert_eq!(editor.cursor(), "界ab\nx".len());
    editor.vertical(-1, true);
    assert_eq!(editor.cursor(), "界ab".len());
    assert_eq!(&editor.text()[editor.selection()], "\nx");
}

#[test]
fn undo_tracks_joined_grapheme_edits_and_discards_redo_after_new_input() {
    let mut editor = Editor::new("👩💻");
    editor.left(false);
    editor.insert("\u{200d}");
    editor.undo();
    assert_eq!(editor.text(), "👩💻");
    editor.redo();
    assert_eq!(editor.text(), "👩‍💻");
    editor.undo();
    editor.insert(" ");
    editor.redo();
    assert_eq!(editor.text(), "👩 💻");
}

#[test]
fn textarea_paste_navigation_and_selection_paint_use_logical_lines() {
    use wove::{elements::Textarea, testing::Screen, Event, Key, Modifiers};
    let mut screen = Screen::new(8, 3);
    let id = screen
        .tree
        .add(screen.tree.root(), Textarea::default())
        .unwrap();
    screen.tree.focus(Some(id)).unwrap();
    screen.send(Event::Paste("a\r\n界b\nc".into())).unwrap();
    screen
        .send(Event::Key(
            Key::Up,
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        ))
        .unwrap();
    let frame = screen.frame().unwrap();
    assert!(frame.cell(0, 1).unwrap().style().reverse);
    assert_eq!(frame.cursor(), Some((0, 1)));
    screen
        .send(Event::Key(
            Key::Char('z'),
            Modifiers {
                ctrl: true,
                ..Modifiers::default()
            },
        ))
        .unwrap();
    assert_eq!(screen.tree.get::<Textarea>(id).unwrap().editor.text(), "");
}

#[test]
fn rich_text_wraps_words_without_splitting_clusters_at_span_boundaries() {
    use wove::{
        elements::RichText,
        testing::Screen,
        text::{Span, Wrap},
    };
    let bold = Style {
        bold: true,
        ..Style::default()
    };
    let mut screen = Screen::new(6, 3);
    let id = screen
        .tree
        .add(
            screen.tree.root(),
            RichText {
                spans: vec![
                    Span::new("hi e", bold),
                    Span::new("\u{301} world", Style::default()),
                ],
                wrap: Wrap::Word,
                ..Default::default()
            },
        )
        .unwrap();
    let mut layout = screen.tree.layout(id).unwrap().clone();
    layout.size.width = wove::layout::length(6.0);
    screen.tree.set_layout(id, layout).unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(frame.cell(3, 0).unwrap().symbol(), "e\u{301}");
    assert!(frame.cell(3, 0).unwrap().style().bold);
    assert_eq!(frame.cell(0, 1).unwrap().symbol(), "w");
}

#[test]
fn logical_line_navigation_never_stops_inside_a_crlf_grapheme() {
    let mut editor = Editor::new("a\r\nb");
    editor.home(false);
    editor.line_end(false);
    assert_eq!(editor.cursor(), 1);
    editor.vertical(1, false);
    assert_eq!(editor.cursor(), 4);
    editor.vertical(-1, false);
    assert_eq!(editor.cursor(), 1);
}

#[test]
fn undo_deletion_restores_the_original_cursor_without_a_selection() {
    let mut editor = Editor::new("a界b");
    editor.left(false);
    let cursor = editor.cursor();
    editor.backspace();
    assert_eq!(editor.text(), "ab");
    editor.undo();
    assert_eq!(editor.text(), "a界b");
    assert_eq!(editor.cursor(), cursor);
    assert!(editor.selection().is_empty());
    editor.redo();
    assert_eq!(editor.text(), "ab");

    editor.home(false);
    editor.delete();
    editor.undo();
    assert_eq!(editor.text(), "ab");
    assert_eq!(editor.cursor(), 0);
    assert!(editor.selection().is_empty());
}

#[test]
fn word_wrap_rechecks_a_wide_grapheme_after_moving_the_word() {
    use wove::{
        elements::RichText,
        testing::Screen,
        text::{Span, TextLayout, Wrap},
    };
    let spans = vec![Span::new(" aab界", Style::default())];
    let layout = TextLayout::new(&spans, Some(4), Wrap::Word);
    assert_eq!(layout.size(), (3, 3));
    let mut screen = Screen::new(4, 3);
    let id = screen
        .tree
        .add(screen.tree.root(), RichText::new(spans, Wrap::Word))
        .unwrap();
    let mut style = screen.tree.layout(id).unwrap().clone();
    style.size.width = wove::layout::length(4.0);
    screen.tree.set_layout(id, style).unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(frame.cell(0, 1).unwrap().symbol(), "a");
    assert_eq!(frame.cell(0, 2).unwrap().symbol(), "界");
}

#[test]
fn clicking_an_input_reports_a_focus_repaint() {
    use wove::{elements::Input, testing::Screen, Button, Event, Mouse, MouseKind};
    let mut screen = Screen::new(10, 2);
    let id = screen
        .tree
        .add(screen.tree.root(), Input::default())
        .unwrap();
    screen.frame().unwrap();
    let event = Event::Mouse(Mouse::new(0, 0, MouseKind::Down(Button::Left)));
    let result = screen.send(event.clone()).unwrap();
    assert_eq!(screen.tree.focused(), Some(id));
    assert!(result.changed);
    screen.frame().unwrap();
    assert!(!screen.send(event).unwrap().changed);
}

#[test]
fn every_input_source_reports_a_shifted_letter_the_same_way() {
    use wove::{input::Decoder, Event, Key, Modifiers};
    let shift = Modifiers {
        shift: true,
        ..Modifiers::default()
    };
    // A local terminal marks `G` with Shift; bytes over SSH do not. A kitty
    // report sends the unshifted key with Shift. All three are one event.
    let local = Event::key(Key::Char('G'), shift);
    let remote = Decoder::default().push(b"G").remove(0);
    let kitty = Decoder::default().push(b"\x1b[103;2u").remove(0);
    assert_eq!(local, Event::Key(Key::Char('G'), Modifiers::default()));
    assert_eq!(remote, local);
    assert_eq!(kitty, local);
    // Keys that are not letters keep Shift: it is all that tells them apart.
    assert_eq!(Event::key(Key::Enter, shift), Event::Key(Key::Enter, shift));
    assert_eq!(
        Event::key(Key::Char('1'), shift),
        Event::Key(Key::Char('1'), shift)
    );
}

#[test]
fn a_typed_word_undoes_as_one_edit_and_the_next_word_as_another() {
    let mut editor = Editor::new("");
    for c in "hello wide".chars() {
        editor.insert(&c.to_string());
    }
    editor.undo();
    assert_eq!(editor.text(), "hello ");
    editor.undo();
    assert_eq!(editor.text(), "");
    editor.redo();
    assert_eq!(editor.text(), "hello ");
}

#[test]
fn words_and_lines_are_units_of_movement_and_deletion() {
    use wove::text::{Command, Motion};
    let mut editor = Editor::new("one two\nthree four");
    editor.apply(Command::Move(Motion::WordLeft, false));
    assert_eq!(editor.cursor(), "one two\nthree ".len());
    editor.apply(Command::Delete(Motion::WordLeft));
    assert_eq!(editor.text(), "one two\nfour");
    editor.apply(Command::Delete(Motion::LineEnd));
    assert_eq!(editor.text(), "one two\n");
    editor.undo();
    editor.undo();
    assert_eq!(editor.text(), "one two\nthree four");
    // Undo puts the cursor back where the deletion started, with no selection.
    assert_eq!(editor.cursor(), "one two\nthree ".len());
    assert!(editor.selection().is_empty());
    editor.apply(Command::Move(Motion::Home, false));
    editor.apply(Command::Move(Motion::WordRight, true));
    assert_eq!(&editor.text()[editor.selection()], "one");
}

#[test]
fn an_atom_is_stepped_over_and_removed_whole_and_undo_brings_it_back() {
    let mut editor = Editor::new("a");
    editor.insert_atom("[paste]", 7);
    editor.insert("b");
    editor.left(false);
    editor.left(false);
    assert_eq!(editor.cursor(), 1, "the cursor never rests inside an atom");
    editor.right(false);
    editor.backspace();
    assert_eq!(editor.text(), "ab");
    assert!(editor.atoms().is_empty());
    editor.undo();
    assert_eq!(editor.text(), "a[paste]b");
    assert_eq!(editor.atoms()[0].id, 7);
    assert_eq!(editor.atoms()[0].range, 1..8);
    editor.home(false);
    editor.insert("xy");
    assert_eq!(
        editor.atoms()[0].range,
        3..10,
        "edits before an atom shift it"
    );
}

#[test]
fn a_wrapping_textarea_breaks_at_words_and_moves_through_display_rows() {
    use wove::{elements::Textarea, testing::Screen, Key};
    let mut screen = Screen::new(6, 4);
    let area = Textarea {
        wrap: true,
        ..Textarea::new("hello wide world")
    };
    let id = screen.tree.add(screen.tree.root(), area).unwrap();
    screen.tree.focus(Some(id)).unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(frame.lines()[..3], ["hello ", "wide  ", "world "]);
    assert_eq!(frame.cursor(), Some((5, 2)));
    screen.send(Key::Up).unwrap();
    assert_eq!(screen.frame().unwrap().cursor(), Some((4, 1)));
    screen.send(Key::Up).unwrap();
    assert_eq!(screen.frame().unwrap().cursor(), Some((5, 0)));
}

#[test]
fn a_click_past_the_end_of_a_wrapped_row_stays_on_that_row() {
    use wove::{elements::Textarea, testing::Screen};
    let mut screen = Screen::new(6, 4);
    let area = Textarea {
        wrap: true,
        ..Textarea::new("hello wide world")
    };
    let id = screen.tree.add(screen.tree.root(), area).unwrap();
    screen.tree.focus(Some(id)).unwrap();
    screen.frame().unwrap();
    // The first row is `hello `; its end is also where the second row starts.
    screen.click(5, 0).unwrap();
    assert_eq!(screen.frame().unwrap().cursor(), Some((5, 0)));
}

#[test]
fn an_atom_is_never_part_of_a_typed_words_undo_step() {
    let mut editor = Editor::new("");
    editor.insert("a");
    editor.insert_atom("@", 1);
    editor.undo();
    assert_eq!(editor.text(), "a");
}

#[test]
fn a_word_wider_than_the_row_breaks_inside_itself_without_overflowing() {
    use wove::text::{wrap, Wrap};
    let source = "ab cdefghij k";
    let rows: Vec<_> = wrap(source, 4, Wrap::Word)
        .into_iter()
        .map(|row| &source[row])
        .collect();
    assert_eq!(rows, ["ab ", "cdef", "ghij ", "k"]);
}
