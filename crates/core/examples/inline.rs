//! An inline session. Finished lines join the terminal's own scrollback while
//! a one-row input stays live beneath them. Enter commits a line; Esc quits.
use wove::{
    elements::{Input, Text},
    terminal::{self, Terminal},
    Event, Id, Key, Options, ScreenMode, Tree,
};

/// Draw the tree at its natural height.
fn draw(terminal: &mut Terminal, tree: &mut Tree) -> Result<(), Box<dyn std::error::Error>> {
    let (width, _) = terminal.size()?;
    let height = tree.height(width)?;
    Ok(terminal.draw(tree.frame(width, height)?)?)
}

/// Show a line above the input, then release it: the terminal keeps the row
/// and the tree forgets it, so the live frame never grows with the session.
fn commit(
    terminal: &mut Terminal,
    tree: &mut Tree,
    input: Id,
    line: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = tree.create(Text::new(line))?;
    tree.insert(tree.root(), text, 0)?;
    draw(terminal, tree)?;
    terminal.commit(1);
    tree.remove(text)?;
    tree.focus(Some(input))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    let input = tree.add(
        tree.root(),
        Input {
            placeholder: "Type a line".into(),
            ..Input::default()
        },
    )?;
    // Leaving the mouse alone keeps the terminal's own text selection.
    let mut terminal = Terminal::with_options(Options {
        screen: ScreenMode::Inline,
        mouse: false,
        ..Options::default()
    })?;
    let header = "Wove inline · Enter commits a line · Esc quits";
    commit(&mut terminal, &mut tree, input, header)?;
    // Keys typed while the session was starting come first.
    let mut typed = terminal.typed_ahead().into_iter();
    loop {
        draw(&mut terminal, &mut tree)?;
        let event = match typed.next() {
            Some(event) => Some(event),
            None => terminal::read()?,
        };
        let Some(event) = event else {
            continue;
        };
        match event {
            Event::Key(Key::Escape, _) => break,
            Event::Key(Key::Enter, _) => {
                let line = tree.get::<Input>(input)?.editor.text().to_owned();
                tree.update::<Input>(input, |input| input.editor.set(""))?;
                commit(&mut terminal, &mut tree, input, &line)?;
            }
            event => {
                tree.dispatch(event)?;
            }
        }
    }
    Ok(())
}
