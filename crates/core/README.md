# Wove

A Rust library for building terminal user interfaces.

Start with a few elements. Arrange them with flex or grid, handle input, and let
Wove draw the terminal. Build directly in Rust, or use the optional Dioxus adapter
for components and signals.

```toml
[dependencies]
wove = { git = "https://github.com/intuitums/wove", branch = "main", version = "0.0.1" }
```

```rust
use wove::{Tree, elements::{Input, Text}, terminal};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut tree = Tree::new();
    tree.add(tree.root(), Text::new("Hello from Wove. What's your name?"))?;
    let input = tree.add(tree.root(), Input::default())?;
    tree.focus(Some(input))?;
    terminal::run(&mut tree, |_, event, _| *event != wove::Key::Escape.into())?;
    Ok(())
}
```

Run your app and start typing. Shift + arrows selects text, Ctrl + Z undoes an
edit, and Escape exits because the callback says so; the library binds no quit key.

Disable default features for headless use. `Tree::frame` returns a cell buffer
without acquiring a terminal. Custom elements implement `Element` and paint through
a clipped `Canvas`. Moving an element preserves its state; removing it drops its
subtree and callbacks.

This is an early release with an unstable API. Current elements are `Container`,
`Panel`, `Text`, `RichText`, `Input`, `Textarea`, `Select`, `List`, `Table`, and
`Scroll`, plus `Feed` for long text documents. `List` requests only visible rows
from its provider, `Scroll` paints only the children in view, and `Feed` lays out
only the blocks in view. `Textarea` scrolls long lines or, with `wrap`, breaks
them at words.

The tree selects text on the painted screen by dragging, orders overlays with
`set_z`, and reports the pointer entering and leaving nodes. A `Terminal` restores
itself on panic and on fatal signals, copies to the clipboard through the
terminal, and probes once at startup for what the terminal supports.

A `Terminal` draws on the alternate screen, over the main screen, or inline:
frames that grow downward from the shell prompt while finished rows stay in the
terminal's own scrollback. `Renderer`, `Inline`, and `input::Decoder` do the same
work on plain bytes, without a terminal, for transports such as SSH.

[Source and examples](https://github.com/intuitums/wove/tree/main) ·
[Guide](https://github.com/intuitums/wove/blob/main/crates/web/src/content/docs/start.mdx)

## Optional features

Only `terminal` is enabled by default. Add any of these independently:

| Feature | API | Purpose |
| --- | --- | --- |
| `markdown` | `wove::markdown::render` | Markdown as styled text. |
| `syntax` | `wove::syntax::Highlighter` | Syntax highlighting with custom grammars and themes. |
| `diff` | `wove::diff::render` | Styled line differences. |

```toml
wove = { git = "https://github.com/intuitums/wove", branch = "main", features = ["markdown"] }
```

All three features work with `default-features = false`, without a terminal backend.
Markdown, syntax highlighting, and diffs return ordinary `RichText` elements;
applications can also construct styled text themselves.

Command bindings and key sequences live in the separate [Keymap package](../keymap).
