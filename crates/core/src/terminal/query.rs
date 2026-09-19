//! Questions the terminal answers on stdin. They are asked once, in one
//! batch, before the application reads input, so replies never race with it.
use crate::{input::Decoder, Event};
use std::io::{self, Write};

/// What the terminal said about itself when the session started.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// The terminal answers the kitty keyboard query, so `Options::keyboard`
    /// takes effect.
    pub keyboard: bool,
    /// The background color, for choosing between light and dark palettes.
    pub background: Option<(u8, u8, u8)>,
}

/// Everything one round of questions produced.
#[derive(Debug, Default, PartialEq)]
pub struct Probe {
    pub capabilities: Capabilities,
    /// Zero-based `(column, row)` of the cursor.
    pub cursor: Option<(u16, u16)>,
    /// Keys that arrived among the replies.
    pub typed: Vec<Event>,
}

/// Ask everything at once. Every terminal answers the device-attributes
/// request sent last, which ends the wait without a timeout even when the
/// other questions go unanswered. Keys typed meanwhile are returned, not lost.
#[cfg(unix)]
pub fn probe(output: &mut impl Write) -> io::Result<Probe> {
    let reply = ask(
        output,
        b"\x1b[?u\x1b]11;?\x1b\\\x1b[6n\x1b[c",
        attributes_end,
    )?;
    Ok(parse(&reply))
}
#[cfg(not(unix))]
pub fn probe(_: &mut impl Write) -> io::Result<Probe> {
    Ok(Probe {
        capabilities: Capabilities {
            // Without a probe the request is sent on trust; terminals ignore it.
            keyboard: true,
            ..Capabilities::default()
        },
        cursor: crossterm::cursor::position().ok(),
        typed: Vec::new(),
    })
}

/// Zero-based `(column, row)` of the cursor, for anchoring an inline session
/// after startup. Input that arrives during the wait is lost, so prefer
/// starting inline.
#[cfg(unix)]
pub fn cursor(output: &mut impl Write) -> io::Result<Option<(u16, u16)>> {
    let reply = ask(output, b"\x1b[6n", |reply| reply.ends_with(b"R"))?;
    Ok(parse(&reply).cursor)
}
#[cfg(not(unix))]
pub fn cursor(_: &mut impl Write) -> io::Result<Option<(u16, u16)>> {
    Ok(crossterm::cursor::position().ok())
}

/// Whether the reply ends with a device-attributes report, `ESC [ ? … c`.
/// A kitty keyboard reply also starts `ESC [ ?`, and a color reply can end in
/// the hex digit `c`, so both ends are checked.
#[cfg_attr(not(unix), allow(dead_code))]
fn attributes_end(reply: &[u8]) -> bool {
    let Some(start) = reply.windows(3).rposition(|w| w == b"\x1b[?") else {
        return false;
    };
    match &reply[start + 3..] {
        [body @ .., b'c'] => body.iter().all(|b| b.is_ascii_digit() || *b == b';'),
        _ => false,
    }
}

/// Write a request and collect stdin until `done` or a short deadline.
#[cfg(unix)]
fn ask(
    output: &mut impl Write,
    request: &[u8],
    done: impl Fn(&[u8]) -> bool,
) -> io::Result<Vec<u8>> {
    use rustix::event::{poll, PollFd, PollFlags, Timespec};
    use std::time::{Duration, Instant};
    output.write_all(request)?;
    output.flush()?;
    let stdin = rustix::stdio::stdin();
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut reply = Vec::new();
    while !done(&reply) && reply.len() < 4096 {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let timeout = Timespec {
            tv_sec: 0,
            tv_nsec: left.subsec_nanos().into(),
        };
        let mut fds = [PollFd::new(&stdin, PollFlags::IN)];
        match poll(&mut fds, Some(&timeout)) {
            Ok(0) => break,
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
        let mut chunk = [0; 256];
        match rustix::io::read(stdin, &mut chunk) {
            Ok(0) => break,
            Ok(n) => reply.extend_from_slice(&chunk[..n]),
            Err(rustix::io::Errno::INTR | rustix::io::Errno::AGAIN) => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(reply)
}

/// Pick the replies out of what arrived; whatever is left was typed.
#[cfg_attr(not(unix), allow(dead_code))]
fn parse(reply: &[u8]) -> Probe {
    let mut capabilities = Capabilities::default();
    let mut cursor = None;
    let mut typed = Vec::new();
    let mut rest = reply;
    while let Some(start) = rest.iter().position(|b| *b == 0x1b) {
        typed.extend_from_slice(&rest[..start]);
        let sequence = &rest[start..];
        // A color reply ends with BEL or ST; every other reply is a CSI.
        let end = if sequence.starts_with(b"\x1b]") {
            let bel = sequence.iter().position(|b| *b == 0x07).map(|i| i + 1);
            let st = sequence.windows(2).skip(1).position(|w| w == b"\x1b\\");
            match (bel, st.map(|i| i + 3)) {
                (Some(bel), Some(st)) => Some(bel.min(st)),
                (bel, st) => bel.or(st),
            }
        } else if sequence.starts_with(b"\x1b[") {
            let last = sequence[2..].iter().position(|b| (0x40..=0x7e).contains(b));
            last.map(|i| i + 3)
        } else {
            // Escape or an Alt key, typed while the replies were arriving.
            // Only that key is input; the replies after it still are replies.
            // An escape byte right after it starts the next sequence instead.
            let key = match sequence.get(1) {
                Some(0x1b) | None => 1,
                Some(_) => 2,
            };
            typed.extend_from_slice(&sequence[..key]);
            rest = &sequence[key..];
            continue;
        };
        let Some(end) = end else {
            typed.extend_from_slice(sequence);
            rest = &[];
            break;
        };
        let body = String::from_utf8_lossy(&sequence[2..end]);
        let is_reply = if let Some(color) = body.strip_prefix("11;rgb:") {
            capabilities.background = channels(color);
            true
        } else if body.starts_with('?') && body.ends_with('u') {
            capabilities.keyboard = true;
            true
        } else if let Some(position) = body.strip_suffix('R') {
            let cell = position.split_once(';').and_then(|(row, column)| {
                let column = column.parse::<u16>().ok()?.checked_sub(1)?;
                Some((column, row.parse::<u16>().ok()?.checked_sub(1)?))
            });
            cursor = cell.or(cursor);
            cell.is_some()
        } else {
            body.starts_with('?') && body.ends_with('c')
        };
        if !is_reply {
            typed.extend_from_slice(&sequence[..end]);
        }
        rest = &sequence[end..];
    }
    typed.extend_from_slice(rest);
    let mut decoder = Decoder::default();
    let mut events = decoder.push(&typed);
    events.extend(decoder.flush_escape());
    Probe {
        capabilities,
        cursor,
        typed: events,
    }
}

/// `RRRR/GGGG/BBBB`, with one to four hex digits per channel.
fn channels(color: &str) -> Option<(u8, u8, u8)> {
    let mut channels = color.split('/').take(3).map(|channel| {
        let digits: String = channel
            .chars()
            .take_while(char::is_ascii_hexdigit)
            .take(4)
            .collect();
        let value = u32::from_str_radix(&digits, 16).ok()?;
        let max = (1u32 << (4 * digits.len() as u32)) - 1;
        Some((value * 255 / max) as u8)
    });
    Some((channels.next()??, channels.next()??, channels.next()??))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    #[test]
    fn replies_are_recognized_and_typed_ahead_keys_survive() {
        let reply = b"l\x1b[?1u\x1b]11;rgb:ffff/8080/1c1c\x1b\\s\x1b[12;40R\x1b[A\x1b[?62;4c";
        assert!(attributes_end(reply));
        let probe = parse(reply);
        assert!(probe.capabilities.keyboard);
        assert_eq!(probe.capabilities.background, Some((255, 128, 28)));
        assert_eq!(probe.cursor, Some((39, 11)));
        let keys = [Key::Char('l').into(), Key::Char('s').into(), Key::Up.into()];
        assert_eq!(probe.typed, keys);
    }

    #[test]
    fn a_terminal_that_answers_only_attributes_has_no_capabilities() {
        assert_eq!(parse(b"\x1b[?62c"), Probe::default());
        // Escape and Alt+x typed mid-probe are keys; the replies stay replies.
        let probe = parse(b"\x1b\x1b]11;rgb:00/00/00\x07\x1bx\x1b[3;1R\x1b[?62c");
        assert_eq!(probe.capabilities.background, Some((0, 0, 0)));
        assert_eq!(probe.cursor, Some((0, 2)));
        assert_eq!(probe.typed.len(), 2);
        // A color reply cut short ends in a hex digit, not in attributes.
        assert!(!attributes_end(b"\x1b[?1u\x1b]11;rgb:1c1c/1c1c/1c1c"));
        assert_eq!(channels("ff/00/7f"), Some((255, 0, 127)));
    }
}
