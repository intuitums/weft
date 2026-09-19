//! Terminal input bytes decoded into portable events, for transports that
//! carry raw bytes instead of a local terminal.
use super::{Button, Event, Key, Modifiers, Mouse, MouseKind};

/// The most paste text delivered as one event. A longer paste arrives as
/// several `Paste` events in order, so memory stays bounded whatever is pasted.
const PASTE_LIMIT: usize = 65536;
/// No report a terminal sends is longer; anything that is gets dropped.
const SEQUENCE_LIMIT: usize = 64;

/// Decode xterm and kitty key reports, SGR mouse reports, focus changes, and
/// bracketed paste. Input may arrive split anywhere; only an incomplete
/// sequence is retained between calls. Malformed input is dropped rather than
/// reported: a peer can send anything, and none of it should end a session.
#[derive(Default)]
pub struct Decoder {
    pending: Vec<u8>,
    paste: bool,
}

impl Decoder {
    /// The input so far is a whole key on its own and also the start of a
    /// longer report: Escape, or Alt with `[` or `O`. Callers wait briefly for
    /// more bytes, then call `flush_escape`.
    pub fn escape_pending(&self) -> bool {
        !self.paste && matches!(self.pending.as_slice(), b"\x1b" | b"\x1b[" | b"\x1bO")
    }

    /// Resolve the waiting bytes as the key they are on their own.
    pub fn flush_escape(&mut self) -> Vec<Event> {
        if !self.escape_pending() {
            return Vec::new();
        }
        let event = match self.pending.get(1) {
            Some(byte) => {
                let alt = Modifiers {
                    alt: true,
                    ..Modifiers::default()
                };
                Event::key(Key::Char(char::from(*byte)), alt)
            }
            None => Key::Escape.into(),
        };
        self.pending.clear();
        vec![event]
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
        let mut events = Vec::new();
        for &byte in bytes {
            self.pending.push(byte);
            if self.paste {
                if self.pending.ends_with(b"\x1b[201~") {
                    let text = &self.pending[..self.pending.len() - 6];
                    events.push(Event::Paste(String::from_utf8_lossy(text).into()));
                    self.pending.clear();
                    self.paste = false;
                } else if self.pending.len() > PASTE_LIMIT {
                    // Deliver what has arrived, keeping enough to recognize a
                    // terminator split across the cut, and cut between characters.
                    // A character is at most four bytes, so three steps back
                    // reach its start; bytes that are not text stop there too.
                    let mut cut = self.pending.len() - 5;
                    let floor = cut - 3;
                    while cut > floor && self.pending[cut] & 0xc0 == 0x80 {
                        cut -= 1;
                    }
                    let text: Vec<u8> = self.pending.drain(..cut).collect();
                    events.push(Event::Paste(String::from_utf8_lossy(&text).into()));
                }
                continue;
            }
            if self.pending == b"\x1b[200~" {
                self.pending.clear();
                self.paste = true;
                continue;
            }
            if self.pending.starts_with(b"\x1b[") || self.pending.starts_with(b"\x1bO") {
                if self.pending.len() > SEQUENCE_LIMIT {
                    self.pending.clear();
                } else if self.pending.len() > 2 && (0x40..=0x7e).contains(&byte) {
                    events.extend(sequence(&self.pending));
                    self.pending.clear();
                }
                continue;
            }
            if self.pending == b"\x1b" {
                continue;
            }
            let alt = self.pending[0] == 0x1b;
            let text = match std::str::from_utf8(&self.pending[usize::from(alt)..]) {
                Ok(text) => text,
                Err(error) if error.error_len().is_none() => continue,
                Err(_) => {
                    self.pending.clear();
                    continue;
                }
            };
            let mut modifiers = Modifiers {
                alt,
                ..Modifiers::default()
            };
            let key = match text.chars().next() {
                Some('\r' | '\n') => Key::Enter,
                Some('\t') => Key::Tab,
                Some('\x7f' | '\x08') => Key::Backspace,
                Some('\x1b') => Key::Escape,
                Some(c @ '\x01'..='\x1a') => {
                    modifiers.ctrl = true;
                    Key::Char(char::from(c as u8 + b'a' - 1))
                }
                Some('\0') => {
                    modifiers.ctrl = true;
                    Key::Char(' ')
                }
                Some(c) => Key::Char(c),
                None => continue,
            };
            events.push(Event::key(key, modifiers));
            self.pending.clear();
        }
        events
    }
}

/// xterm's modifier parameter: one more than a bit mask. Kitty adds super,
/// hyper, and meta, which all report as `meta`.
fn modifiers(parameter: u32) -> Modifiers {
    let mask = parameter.saturating_sub(1);
    Modifiers {
        shift: mask & 1 != 0,
        alt: mask & 2 != 0,
        ctrl: mask & 4 != 0,
        meta: mask & (8 | 16 | 32) != 0,
    }
}

/// Interpret one complete CSI or SS3 report; unsupported reports are ignored.
fn sequence(bytes: &[u8]) -> Option<Event> {
    let end = *bytes.last()?;
    let body = std::str::from_utf8(&bytes[2..bytes.len() - 1]).ok()?;
    if let Some(body) = body.strip_prefix('<') {
        return mouse(body, end);
    }
    // Each `;` field may carry `:` sub-parameters; an omitted value reads as 1.
    let fields: Vec<Vec<u32>> = body
        .split(';')
        .map(|field| field.split(':').map(|n| n.parse().unwrap_or(1)).collect())
        .collect();
    let value = |field: usize, part: usize| fields.get(field)?.get(part).copied();
    if value(1, 1) == Some(3) {
        return None; // A key release.
    }
    let mut modifiers = modifiers(value(1, 0).unwrap_or(1));
    let key = match end {
        b'A' => Key::Up,
        b'B' => Key::Down,
        b'C' => Key::Right,
        b'D' => Key::Left,
        b'H' => Key::Home,
        b'F' => Key::End,
        b'P'..=b'S' => Key::Function(end - b'P' + 1),
        b'Z' => {
            modifiers.shift = true;
            Key::Tab
        }
        b'I' => return Some(Event::WindowFocus(true)),
        b'O' => return Some(Event::WindowFocus(false)),
        b'u' => match value(0, 0)? {
            9 => Key::Tab,
            13 => Key::Enter,
            27 => Key::Escape,
            127 => Key::Backspace,
            // Kitty reports keys without text as private-use code points. The
            // keypad has ordinary meanings; locks, modifiers, media keys, and
            // the rest must never reach an application as characters.
            code @ 57399..=57408 => Key::Char(char::from(b'0' + (code - 57399) as u8)),
            57409 => Key::Char('.'),
            57410 => Key::Char('/'),
            57411 => Key::Char('*'),
            57412 => Key::Char('-'),
            57413 => Key::Char('+'),
            57414 => Key::Enter,
            57415 => Key::Char('='),
            57417 => Key::Left,
            57418 => Key::Right,
            57419 => Key::Up,
            57420 => Key::Down,
            57421 => Key::PageUp,
            57422 => Key::PageDown,
            57423 => Key::Home,
            57424 => Key::End,
            57425 => Key::Insert,
            57426 => Key::Delete,
            0xe000..=0xf8ff => return None,
            code => Key::Char(char::from_u32(code).filter(|c| !c.is_control())?),
        },
        b'~' => match value(0, 0)? {
            1 | 7 => Key::Home,
            2 => Key::Insert,
            3 => Key::Delete,
            4 | 8 => Key::End,
            5 => Key::PageUp,
            6 => Key::PageDown,
            n @ (11..=15) => Key::Function((n - 10) as u8),
            n @ (17..=21) => Key::Function((n - 11) as u8),
            n @ (23..=24) => Key::Function((n - 12) as u8),
            _ => return None,
        },
        _ => return None,
    };
    Some(Event::key(key, modifiers))
}

/// An SGR mouse report: `button;column;row` ending in `M` (press or motion)
/// or `m` (release).
fn mouse(body: &str, end: u8) -> Option<Event> {
    let values: Vec<u16> = body
        .split(';')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let [code, x, y] = values[..] else {
        return None;
    };
    // Buttons past the third set a high bit; they are not a left click.
    if code & 128 != 0 {
        return None;
    }
    let button = match code & 3 {
        0 => Some(Button::Left),
        1 => Some(Button::Middle),
        2 => Some(Button::Right),
        _ => None,
    };
    let kind = match (end, code & 64 != 0, code & 32 != 0) {
        (b'M', true, _) => match code & 3 {
            0 => MouseKind::ScrollUp,
            1 => MouseKind::ScrollDown,
            2 => MouseKind::ScrollLeft,
            _ => MouseKind::ScrollRight,
        },
        (b'M', false, true) => button.map_or(MouseKind::Move, MouseKind::Drag),
        (b'M', false, false) => MouseKind::Down(button?),
        (b'm', false, _) => MouseKind::Up(button?),
        _ => return None,
    };
    Some(Event::Mouse(Mouse {
        x: x.checked_sub(1)?,
        y: y.checked_sub(1)?,
        kind,
        modifiers: Modifiers {
            shift: code & 4 != 0,
            alt: code & 8 != 0,
            ctrl: code & 16 != 0,
            meta: false,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_preserve_unicode_keys_and_paste() {
        let mut decoder = Decoder::default();
        let mut events = Vec::new();
        for byte in "é\x1b[1;5D\x1b[200~a\n\x03\x1b[A\x1b[201~".bytes() {
            events.extend(decoder.push(&[byte]));
        }
        assert_eq!(
            events,
            vec![
                Key::Char('é').into(),
                Event::Key(
                    Key::Left,
                    Modifiers {
                        ctrl: true,
                        ..Modifiers::default()
                    }
                ),
                Event::Paste("a\n\x03\x1b[A".into())
            ]
        );
    }

    #[test]
    fn escape_waits_for_alt_or_timeout() {
        let mut decoder = Decoder::default();
        assert!(decoder.push(b"\x1b").is_empty());
        assert_eq!(decoder.flush_escape(), vec![Key::Escape.into()]);
        assert_eq!(
            decoder.push(b"\x1bx"),
            vec![Event::Key(
                Key::Char('x'),
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                }
            )]
        );
    }

    #[test]
    fn a_huge_paste_arrives_in_order_and_malformed_input_is_dropped() {
        let mut decoder = Decoder::default();
        let body = "é".repeat(PASTE_LIMIT);
        let mut events = decoder.push(b"\x1b[200~");
        events.extend(decoder.push(body.as_bytes()));
        events.extend(decoder.push(b"\x1b[201~x"));
        let pasted: String = events
            .iter()
            .filter_map(|event| match event {
                Event::Paste(text) => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(pasted, body, "chunks never split a character");
        assert!(events.len() > 3);
        assert_eq!(events.last(), Some(&Key::Char('x').into()));
        // A paste of bytes that are not text is cut without walking off its start.
        let mut decoder = Decoder::default();
        decoder.push(b"\x1b[200~");
        let events = decoder.push(&vec![0x80; PASTE_LIMIT + 16]);
        assert!(matches!(events[..], [Event::Paste(_), ..]));
        // A report for a button past the third is not a left click.
        assert!(Decoder::default().push(b"\x1b[<128;1;1M").is_empty());
        // An endless report and invalid UTF-8 are dropped; typing continues.
        let mut decoder = Decoder::default();
        let mut events = decoder.push(format!("\x1b[{}", "1".repeat(80)).as_bytes());
        events.extend(decoder.push(b"\xff"));
        events.extend(decoder.push(b"ok"));
        assert_eq!(
            events[events.len() - 2..],
            [Key::Char('o').into(), Key::Char('k').into()]
        );
    }

    #[test]
    fn alt_with_a_bracket_is_a_key_once_nothing_follows() {
        let mut decoder = Decoder::default();
        assert!(decoder.push(b"\x1b[").is_empty());
        assert!(decoder.escape_pending());
        let alt = Modifiers {
            alt: true,
            ..Modifiers::default()
        };
        assert_eq!(decoder.flush_escape(), [Event::Key(Key::Char('['), alt)]);
        assert_eq!(decoder.push(b"a"), [Key::Char('a').into()]);
        // Bytes that do follow in time still make the report they belong to.
        assert_eq!(decoder.push(b"\x1b[A"), [Key::Up.into()]);
    }

    #[test]
    fn kitty_reports_carry_modifiers_a_legacy_terminal_cannot_send() {
        let mut decoder = Decoder::default();
        // Shift+Enter, Super+v, then a release that must not repeat the key.
        let mut events = decoder.push(b"\x1b[13;2u\x1b[118;9u\x1b[118;9:3u");
        // Keypad Enter is Enter; a media key is not a character.
        assert_eq!(decoder.push(b"\x1b[57414u\x1b[57430u"), [Key::Enter.into()]);
        events.truncate(2);
        assert_eq!(
            events,
            vec![
                Event::Key(
                    Key::Enter,
                    Modifiers {
                        shift: true,
                        ..Modifiers::default()
                    }
                ),
                Event::Key(
                    Key::Char('v'),
                    Modifiers {
                        meta: true,
                        ..Modifiers::default()
                    }
                ),
            ]
        );
    }

    #[test]
    fn mouse_reports_keep_button_drag_and_modifiers() {
        let mut decoder = Decoder::default();
        let events =
            decoder.push(b"\x1b[<2;5;3M\x1b[<34;6;3M\x1b[<2;6;3m\x1b[<35;7;3M\x1b[<80;1;1M");
        let kinds: Vec<_> = events
            .iter()
            .map(|event| match event {
                Event::Mouse(mouse) => (mouse.x, mouse.kind, mouse.modifiers.ctrl),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (4, MouseKind::Down(Button::Right), false),
                (5, MouseKind::Drag(Button::Right), false),
                (5, MouseKind::Up(Button::Right), false),
                (6, MouseKind::Move, false),
                (0, MouseKind::ScrollUp, true),
            ]
        );
    }
}
