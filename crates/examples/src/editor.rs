//! A multiline editor with a live byte count and word-wrapped styled help.
use wove::{
    elements::{RichText, Text, Textarea},
    layout::*,
    terminal,
    text::{Span, Wrap},
    Color, Style, Tree,
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    let root = tree.root();
    tree.set_layout(
        root,
        wove::Layout {
            flex_direction: FlexDirection::Column,
            ..Default::default()
        },
    )?;
    tree.add(
        root,
        RichText {
            spans: vec![
                Span::new(
                    "Wove editor\n",
                    Style {
                        bold: true,
                        fg: Color::Rgb(160, 210, 130),
                        ..Default::default()
                    },
                ),
                Span::new(
                    "Enter adds a line · Shift selects · Ctrl+Z undoes · Esc quits",
                    Style::default(),
                ),
            ],
            wrap: Wrap::Word,
            ..Default::default()
        },
    )?;
    let area = tree.add(
        root,
        Textarea::new("Write something.\nUnicode stays whole: 界 · 👩‍💻"),
    )?;
    tree.set_layout(
        area,
        wove::Layout {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            ..Default::default()
        },
    )?;
    let status = tree.add(root, Text::new("Ready"))?;
    tree.focus(Some(area))?;
    terminal::run(&mut tree, move |tree, event, _| {
        let count = tree
            .get::<Textarea>(area)
            .expect("editor exists")
            .editor
            .text()
            .len();
        tree.update::<Text>(status, |text| text.content = format!("{count} bytes"))
            .expect("status exists");
        *event != wove::Key::Escape.into()
    })?;
    Ok(())
}
