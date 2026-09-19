//! The default key bindings shared by single-line and multiline inputs.
use super::{Command, Editor, Motion};
use crate::{Button, Event, Key, MouseKind, Response};

/// The edit an event stands for under the default bindings. Applications with
/// their own bindings handle keys before the element does and call
/// `Editor::apply` with the commands they choose. Multiline inputs accept
/// newlines and move by line.
pub fn command(event: &Event, multiline: bool) -> Option<Command> {
    let (home, end) = if multiline {
        (Motion::LineHome, Motion::LineEnd)
    } else {
        (Motion::Home, Motion::End)
    };
    Some(match event {
        Event::Paste(value) => Command::Insert(clean(value, multiline)),
        Event::Key(key, m) => {
            let word = m.ctrl || m.alt;
            match key {
                Key::Char('a') if m.ctrl => Command::SelectAll,
                Key::Char('z') if m.ctrl && m.shift => Command::Redo,
                Key::Char('z') if m.ctrl => Command::Undo,
                Key::Char('y') if m.ctrl => Command::Redo,
                Key::Char('e') if m.ctrl => Command::Move(end, false),
                Key::Char('w') if m.ctrl => Command::Delete(Motion::WordLeft),
                Key::Char('k') if m.ctrl => Command::Delete(end),
                Key::Char('u') if m.ctrl => Command::Delete(home),
                Key::Char('b') if m.alt => Command::Move(Motion::WordLeft, m.shift),
                Key::Char('f') if m.alt => Command::Move(Motion::WordRight, m.shift),
                Key::Char('d') if m.alt => Command::Delete(Motion::WordRight),
                Key::Char(c) if !m.ctrl && !m.alt && !m.meta && !c.is_control() => {
                    Command::Insert(c.to_string())
                }
                Key::Enter if multiline => Command::Insert("\n".into()),
                Key::Up if multiline => Command::Move(Motion::Up, m.shift),
                Key::Down if multiline => Command::Move(Motion::Down, m.shift),
                Key::Left if word => Command::Move(Motion::WordLeft, m.shift),
                Key::Right if word => Command::Move(Motion::WordRight, m.shift),
                Key::Left => Command::Move(Motion::Left, m.shift),
                Key::Right => Command::Move(Motion::Right, m.shift),
                Key::Home if m.ctrl => Command::Move(Motion::Home, m.shift),
                Key::End if m.ctrl => Command::Move(Motion::End, m.shift),
                Key::Home => Command::Move(home, m.shift),
                Key::End => Command::Move(end, m.shift),
                Key::Backspace if word => Command::Delete(Motion::WordLeft),
                Key::Delete if word => Command::Delete(Motion::WordRight),
                Key::Backspace => Command::Delete(Motion::Left),
                Key::Delete => Command::Delete(Motion::Right),
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// Apply an event under the default bindings. A left click places the cursor
/// and a drag extends the selection from it.
pub(crate) fn edit(editor: &mut Editor, event: &Event, multiline: bool) -> Response {
    if let Event::Mouse(mouse) = event {
        let extend = match mouse.kind {
            MouseKind::Down(Button::Left) => mouse.modifiers.shift,
            MouseKind::Drag(Button::Left) => true,
            _ => return Response::IGNORE,
        };
        let before = (editor.cursor(), editor.selection());
        editor.seek(editor.position_at(mouse.x, mouse.y), extend);
        return Response {
            handled: true,
            changed: before != (editor.cursor(), editor.selection()),
        };
    }
    match command(event, multiline) {
        Some(command) => {
            editor.apply(command);
            Response::CHANGED
        }
        None => Response::IGNORE,
    }
}

/// Normalize pasted newlines, expand tabs, and remove terminal controls before
/// storing input.
pub(crate) fn clean(value: &str, multiline: bool) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\t', &" ".repeat(crate::render::TAB))
        .chars()
        .filter(|c| !c.is_control() || (multiline && *c == '\n'))
        .collect()
}
