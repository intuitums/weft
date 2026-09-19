//! Ownership of the local terminal, kept separate from pure drawing.
use crate::{Buffer, Depth, Inline, Renderer};
pub use crate::{Options, ScreenMode};
pub use crossterm::event::EventStream;
use crossterm::{event, terminal};
use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, Once};

mod query;
pub use query::Capabilities;

static OWNED: AtomicBool = AtomicBool::new(false);
/// What to undo if the process ends without dropping the session.
struct Active {
    options: Options,
    /// Puts the cursor beneath an inline frame, so that what prints next
    /// lands under the frame instead of over it.
    park: Vec<u8>,
}

/// Whoever takes this, the session, the panic hook, or the signal thread,
/// restores the terminal, so it happens exactly once.
static ACTIVE: Mutex<Option<Active>> = Mutex::new(None);

/// A panic elsewhere must not stop the terminal from being restored, so a
/// poisoned lock is used like any other.
fn active() -> std::sync::MutexGuard<'static, Option<Active>> {
    ACTIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Restore the terminal from wherever the process is ending.
fn rescue() {
    let taken = active().take();
    if let Some(Active { options, park }) = taken {
        let mut output = io::stdout();
        let _ = output.write_all(&park);
        let _ = options.leave(&mut output);
        let _ = terminal::disable_raw_mode();
    }
}

/// What the signal thread does when a fatal signal arrives.
static SIGNALS: AtomicU8 = AtomicU8::new(DEFAULT);
/// No session is active: the signal has its usual effect.
const DEFAULT: u8 = 0;
/// A session asked for rescue: restore the terminal, then the usual effect.
const RESCUE: u8 = 1;
/// A session handles signals itself: stay out of the way.
const HANDS_OFF: u8 = 2;

/// A panic message printed onto the alternate screen vanishes with it, and one
/// printed in raw mode staircases. Restore first, then let the message print.
fn install_panic_hook() {
    static HOOK: Once = Once::new();
    HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            rescue();
            previous(info);
        }));
    });
}

/// A killed process runs no destructors, which would leave the user's shell
/// in raw mode with mouse reports pouring in. Restore, then let the signal
/// have its usual effect.
///
/// Registering for a signal replaces its default action for the life of the
/// process, so the thread stays once started and consults `SIGNALS`: it still
/// ends the process when no session is active, and does nothing while a
/// session that handles signals itself is.
#[cfg(unix)]
fn watch_signals() {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
    static WATCH: Once = Once::new();
    WATCH.call_once(|| {
        let Ok(mut signals) =
            signal_hook::iterator::Signals::new([SIGHUP, SIGINT, SIGQUIT, SIGTERM])
        else {
            return;
        };
        std::thread::spawn(move || {
            for signal in signals.forever() {
                if SIGNALS.load(Ordering::Acquire) == HANDS_OFF {
                    continue;
                }
                rescue();
                let _ = signal_hook::low_level::emulate_default_handler(signal);
            }
        });
    });
}
#[cfg(not(unix))]
fn watch_signals() {}

/// Owns the terminal session. Drop restores modes on normal return; a panic
/// and, unless `Options::signals` is off, a fatal signal restore them too.
/// Only an abort or an uncatchable signal leaves them behind. Only one session
/// may exist; callers must not change terminal modes behind it.
///
/// A session is `Send`. An application whose terminal can be slow, over SSH
/// or inside a multiplexer, can draw from a thread of its own and hand it
/// cloned frames, so a blocked write never stalls input handling.
pub struct Terminal {
    output: io::Stdout,
    options: Options,
    depth: Depth,
    renderer: Renderer,
    inline: Option<Inline>,
    capabilities: Capabilities,
    /// Where the cursor was when the session started, until an inline frame
    /// is anchored there.
    start: Option<(u16, u16)>,
    typed: Vec<crate::Event>,
    probed: bool,
    raw: bool,
    entered: bool,
}

impl Terminal {
    /// Acquire an interactive stdin/stdout terminal and enter the alternate screen.
    /// Existing raw sessions are rejected rather than taking over their modes.
    pub fn new() -> io::Result<Self> {
        Self::with_options(Options::default())
    }

    pub fn with_mode(screen: ScreenMode) -> io::Result<Self> {
        Self::with_options(Options {
            screen,
            ..Options::default()
        })
    }

    /// Colors are mapped to what the environment says the terminal can show.
    /// The terminal is asked once, here, what it supports and where its cursor
    /// is; see `capabilities`.
    pub fn with_options(options: Options) -> io::Result<Self> {
        if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
            return Err(io::Error::other(
                "wove needs an interactive stdin and stdout",
            ));
        }
        if OWNED
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(io::Error::other("a wove terminal is already active"));
        }
        let depth = Depth::detect();
        let mut session = Self {
            output: io::stdout(),
            options,
            depth,
            renderer: Renderer::with_depth(depth),
            inline: None,
            capabilities: Capabilities::default(),
            start: None,
            typed: Vec::new(),
            probed: false,
            raw: false,
            entered: false,
        };
        if terminal::is_raw_mode_enabled()? {
            return Err(io::Error::other("terminal is already in raw mode"));
        }
        install_panic_hook();
        if options.signals {
            watch_signals();
        }
        let signals = if options.signals { RESCUE } else { HANDS_OFF };
        SIGNALS.store(signals, Ordering::Release);
        session.resume()?;
        Ok(session)
    }

    /// What the terminal reported at startup.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Keys typed while the terminal was being probed. Handle them before
    /// reading input, so typing ahead of a slow start is not lost.
    pub fn typed_ahead(&mut self) -> Vec<crate::Event> {
        std::mem::take(&mut self.typed)
    }

    /// Restore terminal modes temporarily while retaining exclusive ownership.
    /// Resume before drawing again. This permits an application to run a child
    /// UI. An inline session first parks the cursor beneath its last frame.
    pub fn suspend(&mut self) -> io::Result<()> {
        let mut result = Ok(());
        // Whoever holds the modes undoes them; a panic hook may have already.
        let taken = active().take();
        if self.entered && taken.is_some() {
            if self.options.screen == ScreenMode::Inline {
                if let Some(inline) = &mut self.inline {
                    result = inline.finish(&mut self.output);
                }
            }
            result = result.and(self.options.leave(&mut self.output));
        }
        self.entered = false;
        if self.raw {
            let raw = terminal::disable_raw_mode();
            self.raw = raw.is_err();
            result = result.and(raw);
        }
        self.invalidate();
        result
    }

    /// Reacquire modes after suspension and force a complete repaint.
    pub fn resume(&mut self) -> io::Result<()> {
        if self.raw && self.entered {
            return Ok(());
        }
        if !self.raw {
            terminal::enable_raw_mode()?;
            self.raw = true;
        }
        // Raw escape sequences need virtual terminal processing on Windows.
        #[cfg(windows)]
        let _ = crossterm::ansi_support::supports_ansi();
        if !self.probed {
            self.probed = true;
            let probe = query::probe(&mut self.output)?;
            self.capabilities = probe.capabilities;
            self.start = probe.cursor;
            self.typed = probe.typed;
            // A request for key reports the terminal never answered is dropped.
            self.options.keyboard &= self.capabilities.keyboard;
        }
        self.entered = true;
        self.renderer.invalidate();
        match &mut self.inline {
            // Whatever ran during the suspension may have written anywhere.
            Some(inline) => inline.invalidate(),
            None if self.options.screen == ScreenMode::Inline => {
                let cursor = self.start.take();
                self.anchor(cursor)?;
            }
            None => {}
        }
        self.options.enter(&mut self.output)?;
        self.record();
        Ok(())
    }

    /// Keep what a crash would need to restore the terminal up to date.
    fn record(&self) {
        let park = match (&self.inline, self.options.screen) {
            (Some(inline), ScreenMode::Inline) => inline.park(),
            (_, ScreenMode::Alternate) => Vec::new(),
            _ => b"\r\n".to_vec(),
        };
        *active() = Some(Active {
            options: self.options,
            park,
        });
    }

    /// Map an event from the screen onto the frame. An inline frame starts
    /// wherever the session began and moves as it scrolls, so a mouse report's
    /// row is not the frame's row. Pass every event through this before
    /// dispatching it; one that falls outside an inline frame is `None`.
    pub fn to_frame(&self, event: crate::Event) -> Option<crate::Event> {
        match (event, &self.inline, self.options.screen) {
            (crate::Event::Mouse(mouse), Some(inline), ScreenMode::Inline) => {
                let y = inline.frame_row(mouse.y)?;
                Some(crate::Event::Mouse(crate::Mouse { y, ..mouse }))
            }
            (event, ..) => Some(event),
        }
    }

    /// Stop the process as Ctrl-Z does in a shell, and come back when it is
    /// continued. Raw mode turns the key into an ordinary event, so an
    /// application that wants job control calls this when it sees one.
    #[cfg(unix)]
    pub fn stop(&mut self) -> io::Result<()> {
        self.suspend()?;
        signal_hook::low_level::raise(signal_hook::consts::SIGTSTP)?;
        self.resume()
    }

    /// Turn the input modes on again. Windows consoles drop them while the
    /// window is unfocused, so call this on `Event::WindowFocus(true)` there.
    pub fn reassert(&mut self) -> io::Result<()> {
        if self.entered {
            self.options.reassert(&mut self.output)?;
        }
        Ok(())
    }

    /// Ask the terminal to put text on the system clipboard. It works over
    /// SSH and through tmux, and does nothing where the terminal declines.
    pub fn copy(&mut self, text: &str) -> io::Result<()> {
        let tmux = std::env::var_os("TMUX").is_some();
        self.output
            .write_all(&crate::render::clipboard(text, tmux))?;
        self.output.flush()
    }

    /// Start the inline region on a fresh line at the cursor. A terminal that
    /// does not report its cursor gets a cleared screen instead.
    fn anchor(&mut self, cursor: Option<(u16, u16)>) -> io::Result<()> {
        let row = match cursor {
            Some((column, row)) if column > 0 => {
                self.output.write_all(b"\r\n")?;
                row.saturating_add(1)
            }
            Some((_, row)) => row,
            None => 0,
        };
        let mut inline = Inline::new(row, self.depth);
        if cursor.is_none() {
            inline.invalidate();
        }
        self.inline = Some(inline);
        Ok(())
    }

    /// Move between screens while running, for example from an inline session
    /// to a full-screen view and back. The main screen keeps what the inline
    /// session drew while the alternate screen is up. Start inline when the
    /// session will be inline at all: anchoring later has to ask the terminal
    /// for its cursor, and input that arrives during the wait is lost.
    pub fn switch(&mut self, screen: ScreenMode) -> io::Result<()> {
        let old = self.options.screen;
        if old == screen {
            return Ok(());
        }
        self.options.screen = screen;
        self.renderer.invalidate();
        if !self.entered {
            return Ok(());
        }
        // The main and alternate screens keep separate stacks of keyboard
        // modes, so ours is popped from the screen being left and pushed on the
        // one being entered. Otherwise the pop at exit misses, and the shell
        // inherits key reports it cannot read.
        let crossing = old == ScreenMode::Alternate || screen == ScreenMode::Alternate;
        if crossing && self.options.keyboard {
            self.output.write_all(b"\x1b[<1u")?;
        }
        if old == ScreenMode::Alternate {
            self.output.write_all(b"\x1b[?1049l")?;
        }
        if screen == ScreenMode::Alternate {
            self.output.write_all(b"\x1b[?1049h")?;
        }
        if crossing && self.options.keyboard {
            self.output.write_all(b"\x1b[>1u")?;
        }
        self.output.flush()?;
        match (&mut self.inline, screen, old) {
            (None, ScreenMode::Inline, _) => {
                let cursor = query::cursor(&mut self.output)?;
                self.anchor(cursor)?;
            }
            (Some(inline), ScreenMode::Inline, ScreenMode::Main) => inline.invalidate(),
            _ => {}
        }
        self.record();
        Ok(())
    }

    /// Current terminal dimensions; read again after resize events.
    pub fn size(&self) -> io::Result<(u16, u16)> {
        terminal::size()
    }

    /// Paint a frame. A full-screen frame matches the terminal's dimensions.
    /// An inline frame matches its width and may be of any height.
    pub fn draw(&mut self, frame: &Buffer) -> io::Result<()> {
        if !self.raw || !self.entered {
            return Err(io::Error::other("terminal is suspended"));
        }
        match (&mut self.inline, self.options.screen) {
            (Some(inline), ScreenMode::Inline) => {
                inline.draw(&mut self.output, frame, terminal::size()?.1)?;
                // The frame may have grown or scrolled; a crash parks beneath it.
                self.record();
                Ok(())
            }
            _ => self.renderer.draw(&mut self.output, frame),
        }
    }

    /// Inline only: release the first `rows` rows of the last frame to the
    /// terminal's history. See `Inline::commit`.
    pub fn commit(&mut self, rows: u16) {
        if let Some(inline) = &mut self.inline {
            inline.commit(rows);
        }
    }

    /// Repaint on the next draw after another owner wrote to stdout.
    pub fn invalidate(&mut self) {
        self.renderer.invalidate();
        if let Some(inline) = &mut self.inline {
            inline.invalidate();
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.suspend();
        SIGNALS.store(DEFAULT, Ordering::Release);
        OWNED.store(false, Ordering::Release);
    }
}

/// Terminal events are converted here so core elements never depend on crossterm.
pub fn read() -> io::Result<Option<crate::Event>> {
    Ok(convert(event::read()?))
}

/// Convert backend input for synchronous readers and asynchronous event streams.
pub fn convert(input: event::Event) -> Option<crate::Event> {
    use crate::{Button, Event, Key, Modifiers, Mouse, MouseKind};
    let modifiers = |m: event::KeyModifiers| Modifiers {
        ctrl: m.contains(event::KeyModifiers::CONTROL),
        alt: m.contains(event::KeyModifiers::ALT),
        shift: m.contains(event::KeyModifiers::SHIFT),
        meta: m.intersects(
            event::KeyModifiers::SUPER | event::KeyModifiers::META | event::KeyModifiers::HYPER,
        ),
    };
    match input {
        event::Event::Key(key) if key.kind != event::KeyEventKind::Release => {
            let mut mods = modifiers(key.modifiers);
            let key = match key.code {
                event::KeyCode::Char(c) => Key::Char(c),
                event::KeyCode::Enter => Key::Enter,
                event::KeyCode::Esc => Key::Escape,
                event::KeyCode::Tab => Key::Tab,
                event::KeyCode::BackTab => {
                    mods.shift = true;
                    Key::Tab
                }
                event::KeyCode::Backspace => Key::Backspace,
                event::KeyCode::Delete => Key::Delete,
                event::KeyCode::Left => Key::Left,
                event::KeyCode::Right => Key::Right,
                event::KeyCode::Up => Key::Up,
                event::KeyCode::Down => Key::Down,
                event::KeyCode::Home => Key::Home,
                event::KeyCode::End => Key::End,
                event::KeyCode::PageUp => Key::PageUp,
                event::KeyCode::PageDown => Key::PageDown,
                event::KeyCode::Insert => Key::Insert,
                event::KeyCode::F(number) => Key::Function(number),
                _ => return None,
            };
            Some(Event::key(key, mods))
        }
        event::Event::Paste(s) => Some(Event::Paste(s)),
        event::Event::Mouse(m) => {
            let button = |button| match button {
                event::MouseButton::Left => Button::Left,
                event::MouseButton::Middle => Button::Middle,
                event::MouseButton::Right => Button::Right,
            };
            let kind = match m.kind {
                event::MouseEventKind::Down(b) => MouseKind::Down(button(b)),
                event::MouseEventKind::Up(b) => MouseKind::Up(button(b)),
                event::MouseEventKind::Drag(b) => MouseKind::Drag(button(b)),
                event::MouseEventKind::Moved => MouseKind::Move,
                event::MouseEventKind::ScrollUp => MouseKind::ScrollUp,
                event::MouseEventKind::ScrollDown => MouseKind::ScrollDown,
                event::MouseEventKind::ScrollLeft => MouseKind::ScrollLeft,
                event::MouseEventKind::ScrollRight => MouseKind::ScrollRight,
            };
            Some(Event::Mouse(Mouse {
                x: m.column,
                y: m.row,
                kind,
                modifiers: modifiers(m.modifiers),
            }))
        }
        event::Event::Resize(width, height) => Some(Event::Resize(width, height)),
        event::Event::FocusGained => Some(Event::WindowFocus(true)),
        event::Event::FocusLost => Some(Event::WindowFocus(false)),
        _ => None,
    }
}

/// Wait without reading. Custom loops can multiplex terminal input and their own work.
pub fn poll(timeout: std::time::Duration) -> io::Result<bool> {
    event::poll(timeout)
}

/// Run a tree on the alternate screen, calling `update` after each dispatched
/// event. The loop ends when `update` returns false; which keys quit is the
/// application's decision, so handle one or the terminal stays captured.
pub fn run(
    tree: &mut crate::Tree,
    mut update: impl FnMut(&mut crate::Tree, &crate::Event, &crate::Dispatch) -> bool,
) -> io::Result<()> {
    let mut terminal = Terminal::new()?;
    loop {
        let (w, h) = terminal.size()?;
        terminal.draw(tree.frame(w, h).map_err(io::Error::other)?)?;
        let Some(event) = read()? else {
            continue;
        };
        let result = tree.dispatch(event.clone()).map_err(io::Error::other)?;
        if !update(tree, &event, &result) {
            break;
        }
    }
    Ok(())
}
