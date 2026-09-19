//! Local frame timings. No pass/fail thresholds.
use std::{hint::black_box, time::Instant};
use wove::{
    elements::{Feed, Scroll, Text},
    text::{Span, Wrap},
    Renderer, Style, Tree,
};

/// Milliseconds per iteration.
fn time(iterations: u32, mut work: impl FnMut(u32)) -> f64 {
    let start = Instant::now();
    for i in 0..iterations {
        work(i);
    }
    start.elapsed().as_secs_f64() * 1000.0 / f64::from(iterations)
}

fn main() {
    let mut tree = Tree::new();
    let text = tree.add(tree.root(), Text::new("wove")).unwrap();
    tree.frame(100, 30).unwrap();
    let changed = time(1000, |i| {
        tree.update::<Text>(text, |w| w.content = format!("Frame {i}\nUnicode: 界 👩‍💻"))
            .unwrap();
        black_box(tree.frame(100, 30).unwrap());
    });
    let idle = time(100_000, |_| {
        black_box(tree.frame(100, 30).unwrap());
    });
    println!("changed frame: {changed:.4} ms\ncached frame:  {idle:.6} ms");

    // Terminal output for a large styled screen: everything, one row, nothing.
    let mut tree = Tree::new();
    let rows: Vec<_> = (0..60)
        .map(|y| {
            let text = Text {
                style: wove::Style {
                    fg: wove::Color::Indexed(y as u8),
                    bold: y % 2 == 0,
                    ..Default::default()
                },
                ..Text::new("the quick brown fox ".repeat(10))
            };
            tree.add(tree.root(), text).unwrap()
        })
        .collect();
    let mut renderer = Renderer::default();
    let mut sink = Vec::new();
    let full = time(500, |_| {
        renderer.invalidate();
        sink.clear();
        renderer
            .draw(&mut sink, tree.frame(200, 60).unwrap())
            .unwrap();
    });
    let one = time(2000, |i| {
        tree.update::<Text>(rows[30], |w| w.content = format!("row {i}"))
            .unwrap();
        sink.clear();
        renderer
            .draw(&mut sink, tree.frame(200, 60).unwrap())
            .unwrap();
    });
    let none = time(2000, |_| {
        renderer
            .draw(&mut sink, tree.frame(200, 60).unwrap())
            .unwrap();
    });
    println!("200x60 full repaint:   {full:.4} ms");
    println!("200x60 one row changed: {one:.4} ms");
    println!("200x60 nothing changed: {none:.4} ms");

    // A long session: many wrapped blocks in a scroll that follows its tail,
    // with the last block growing each frame and output written to a sink.
    let mut tree = Tree::new();
    let scroll = tree.add(tree.root(), Scroll::default()).unwrap();
    tree.update::<Scroll>(scroll, |s| s.follow = true).unwrap();
    let mut last = scroll;
    for i in 0..10_000 {
        let block = Text {
            wrap: true,
            ..Text::new(format!("Block {i}: {}", "lorem ipsum dolor ".repeat(12)))
        };
        last = tree.add(scroll, block).unwrap();
        let mut layout = tree.layout(last).unwrap().clone();
        layout.flex_shrink = 0.0;
        tree.set_layout(last, layout).unwrap();
    }
    let mut renderer = Renderer::default();
    sink.clear();
    let first = time(1, |_| {
        renderer
            .draw(&mut sink, tree.frame(100, 30).unwrap())
            .unwrap();
    });
    let streaming = time(200, |_| {
        tree.update::<Text>(last, |w| w.content.push_str("more words "))
            .unwrap();
        sink.clear();
        renderer
            .draw(&mut sink, tree.frame(100, 30).unwrap())
            .unwrap();
    });
    println!("10,000 blocks, first frame:     {first:.3} ms");
    println!("10,000 blocks, streaming frame: {streaming:.3} ms");

    // The same session in a feed, which lays out only the blocks in view.
    let block = |text: String| vec![Span::new(text, Style::default())];
    let mut tree = Tree::new();
    let mut feed = Feed::new(1);
    let mut last = 0;
    for i in 0..10_000 {
        let text = format!("Block {i}: {}", "lorem ipsum dolor ".repeat(12));
        last = feed.push(block(text), Wrap::Word);
    }
    let feed = tree.add(tree.root(), feed).unwrap();
    let mut renderer = Renderer::default();
    let first = time(1, |_| {
        renderer
            .draw(&mut sink, tree.frame(100, 30).unwrap())
            .unwrap();
    });
    let mut text = "Streaming: ".to_owned();
    let streaming = time(200, |_| {
        text.push_str("more words ");
        tree.update::<Feed>(feed, |feed| feed.set(last, block(text.clone())))
            .unwrap();
        sink.clear();
        renderer
            .draw(&mut sink, tree.frame(100, 30).unwrap())
            .unwrap();
    });
    println!("10,000 block feed, first frame:     {first:.3} ms");
    println!("10,000 block feed, streaming frame: {streaming:.3} ms");
}
