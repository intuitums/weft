use std::{cell::Cell, rc::Rc};
use wove::{
    elements::{Feed, List, Table},
    testing::Screen,
    text::{Span, Wrap},
    Key, Style,
};

fn block(text: &str) -> Vec<Span> {
    vec![Span::new(text, Style::default())]
}

#[test]
fn a_feed_follows_its_tail_until_scrolled_and_holds_its_place_while_blocks_arrive() {
    let mut screen = Screen::new(6, 3);
    let mut feed = Feed::new(0);
    for i in 0..100_000 {
        feed.push(block(&format!("b{i}")), Wrap::Word);
    }
    let id = screen.tree.add(screen.tree.root(), feed).unwrap();
    screen.tree.focus(Some(id)).unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["b99997", "b99998", "b99999"]
    );
    screen.send(Key::Up).unwrap();
    screen
        .tree
        .update::<Feed>(id, |feed| {
            feed.push(block("new"), Wrap::Word);
        })
        .unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["b99996", "b99997", "b99998"]
    );
    assert!(!screen.tree.get::<Feed>(id).unwrap().following());
    screen.send(Key::PageDown).unwrap();
    assert!(screen.tree.get::<Feed>(id).unwrap().following());
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["b99998", "b99999", "new   "]
    );
}

#[test]
fn a_feed_scrolls_by_rows_through_wrapped_blocks_and_their_gaps() {
    let mut screen = Screen::new(6, 3);
    let mut feed = Feed::new(1);
    feed.push(block("top"), Wrap::Word);
    let last = feed.push(block("aaaaaa bbbbbb"), Wrap::Word);
    let id = screen.tree.add(screen.tree.root(), feed).unwrap();
    screen.tree.focus(Some(id)).unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["      ", "aaaaaa", "bbbbbb"]
    );
    screen.send(Key::Up).unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["top   ", "      ", "aaaaaa"]
    );
    // Growing the last block while scrolled away does not move the view.
    screen
        .tree
        .update::<Feed>(id, |feed| feed.set(last, block("aaaaaa bbbbbb cccccc")))
        .unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["top   ", "      ", "aaaaaa"]
    );
    screen.send(Key::End).unwrap();
    assert_eq!(
        screen.frame().unwrap().lines(),
        ["aaaaaa", "bbbbbb", "cccccc"]
    );
}

#[test]
fn a_feed_anchored_in_a_tall_block_keeps_showing_text_when_it_gets_wider() {
    let mut screen = Screen::new(4, 2);
    let mut feed = Feed::new(0);
    feed.push(block("aaaa bbbb cccc dddd eeee"), Wrap::Word);
    feed.push(block("tail"), Wrap::Word);
    feed.push(block("end"), Wrap::Word);
    let id = screen.tree.add(screen.tree.root(), feed).unwrap();
    screen.tree.focus(Some(id)).unwrap();
    screen.frame().unwrap();
    screen.send(Key::Up).unwrap();
    screen.send(Key::Up).unwrap();
    assert_eq!(screen.frame().unwrap().lines(), ["dddd", "eeee"]);
    // At this width the first block is one row; the anchor must not point past it.
    screen.resize(30, 2);
    let lines = screen.frame().unwrap().lines();
    assert!(
        lines[0].starts_with("aaaa bbbb cccc dddd eeee"),
        "{lines:?}"
    );
    assert!(lines[1].starts_with("tail"), "{lines:?}");
}

#[test]
fn million_row_list_materializes_only_the_visible_viewport() {
    let calls = Rc::new(Cell::new(0));
    let count = calls.clone();
    let mut screen = Screen::new(12, 4);
    let id = screen
        .tree
        .add(
            screen.tree.root(),
            List::new(1_000_000, 12, move |i| {
                count.set(count.get() + 1);
                vec![Span::new(format!("Row {i}"), Style::default())]
            }),
        )
        .unwrap();
    screen.tree.focus(Some(id)).unwrap();
    screen.frame().unwrap();
    assert_eq!(calls.get(), 4);
    screen.send(Key::End).unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(calls.get(), 8);
    assert!(frame.lines()[3].contains("999999"));
}
#[test]
fn table_preserves_header_and_clips_wide_cells_before_next_column() {
    let mut screen = Screen::new(8, 3);
    let id = screen
        .tree
        .add(
            screen.tree.root(),
            Table::new(
                vec![("A".into(), 3), ("B".into(), 3)],
                vec![
                    vec!["界界".into(), "one".into()],
                    vec!["x".into(), "two".into()],
                    vec!["y".into(), "end".into()],
                ],
            ),
        )
        .unwrap();
    screen.tree.focus(Some(id)).unwrap();
    assert_eq!(screen.frame().unwrap().cell(4, 1).unwrap().symbol(), "o");
    screen.send(Key::End).unwrap();
    let frame = screen.frame().unwrap();
    assert_eq!(frame.cell(0, 0).unwrap().symbol(), "A");
    assert!(frame.lines()[2].contains("end"));
}
