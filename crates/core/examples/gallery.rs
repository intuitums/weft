//! A small catalogue demonstrates direct tree ownership and independent elements.
use wove::{elements::*, layout::*, terminal, Color, Layout, Style, Tree};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    let title = tree.add(tree.root(), Text::new("Wove · element gallery"))?;
    tree.update::<Text>(title, |w| {
        w.style = Style {
            fg: Color::Rgb(104, 211, 192),
            bold: true,
            ..Style::default()
        }
    })?;
    let input = tree.add(
        tree.root(),
        Input {
            placeholder: "Filter elements".into(),
            ..Input::default()
        },
    )?;
    let body = tree.add(tree.root(), Container)?;
    tree.set_layout(
        body,
        Layout {
            flex_grow: 1.0,
            min_size: Size {
                width: length(0.0),
                height: length(0.0),
            },
            gap: Size {
                width: length(2.0),
                height: length(0.0),
            },
            ..Layout::default()
        },
    )?;
    let names = ["Text", "Input", "Select", "Scroll", "Panel"];
    let list = tree.add(body, Select::new(names))?;
    tree.set_layout(
        list,
        Layout {
            size: Size {
                width: length(16.0),
                height: percent(1.0),
            },
            flex_shrink: 0.0,
            ..Layout::default()
        },
    )?;
    let panel = tree.add(body, Panel::default())?;
    let mut style = tree.layout(panel)?.clone();
    style.flex_grow = 1.0;
    style.min_size.width = length(0.0);
    tree.set_layout(panel, style)?;
    let scroll = tree.add(panel, Scroll::default())?;
    let mut style = tree.layout(scroll)?.clone();
    style.flex_grow = 1.0;
    style.min_size.height = length(0.0);
    tree.set_layout(scroll, style)?;
    let detail = tree.add(
        scroll,
        Text {
            content: description("Text"),
            wrap: true,
            ..Text::default()
        },
    )?;
    let mut style = tree.layout(detail)?.clone();
    style.flex_shrink = 0.0;
    tree.set_layout(detail, style)?;
    tree.add(
        tree.root(),
        Text::new("Tab focus · arrows select / scroll · Esc quit"),
    )?;
    tree.focus(Some(input))?;
    terminal::run(&mut tree, |tree, event, _| {
        let filter = tree
            .get::<Input>(input)
            .expect("input exists")
            .editor
            .text()
            .to_lowercase();
        let items: Vec<String> = names
            .iter()
            .filter(|name| name.to_lowercase().contains(&filter))
            .map(|s| s.to_string())
            .collect();
        if tree.get::<Select>(list).expect("list exists").items != items {
            tree.update::<Select>(list, |w| {
                w.items = items;
                w.selected = 0;
            })
            .expect("list exists");
        }
        let selected = tree.get::<Select>(list).expect("list exists");
        let content = selected
            .items
            .get(selected.selected)
            .map_or_else(|| "No matching elements".into(), |s| description(s));
        if tree.get::<Text>(detail).expect("detail exists").content != content {
            tree.update::<Text>(detail, |w| w.content = content)
                .expect("detail exists");
            tree.update::<Scroll>(scroll, |w| {
                w.offset = 0;
                w.follow = false;
            })
            .expect("scroll exists");
        }
        *event != wove::Key::Escape.into()
    })?;
    Ok(())
}
fn description(name: &str) -> String {
    let body=match name {
        "Text"=>"Unicode text measured and painted with the same wrapping rules.\n\nGraphemes stay whole: café · 界.\n\nResize the terminal to see layout recompute.",
        "Input"=>"Editable text with grapheme movement, selection, and undo.\n\nShift + arrows selects. Ctrl + A selects all. Ctrl + Z undoes. Ctrl + Y redoes.\n\nPaste inserts text without terminal controls.",
        "Select"=>"A list with keyboard selection and a visible selected row.\n\nApplications own item meaning and choose the highlight style.",
        "Scroll"=>"A clipped viewport.\n\nArrow keys move one row. Page Up and Page Down move a page. End follows new content. Moving away from the end stops following.",
        _=>"A bordered container with one cell reserved on each edge.\n\nPut any elements inside. Layout determines their size and placement.",
    };
    format!("{name}\n\n{body}")
}
