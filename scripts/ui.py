"""Exercise input, resize, and terminal restoration through real Unix PTYs."""
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import subprocess
import sys
import termios
import time
import pyte

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "artifacts/ui"
OUT.mkdir(parents=True, exist_ok=True)


def scenario(name, steps, fullscreen=True):
    """Run an example on a PTY and retain visible frames and terminal bytes.

    An inline example stays on the main screen; its rows must stay in order and
    it is not resized, because a resize clears the rows it has released.
    """
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
    original = termios.tcgetattr(slave)
    screen = pyte.Screen(80, 24)
    stream = pyte.ByteStream(screen)
    raw = bytearray()
    binary = ROOT / "target/debug" / name if name == "editor" else ROOT / "target/debug/examples" / name
    process = subprocess.Popen([str(binary)],
                               stdin=slave, stdout=slave, stderr=slave,
                               start_new_session=True, env={**os.environ, "TERM": "xterm-256color"})

    def receive(expected, after=-1):
        """Drain complete output until the expected frame appears or time expires."""
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if select.select([master], [], [], 0.05)[0]:
                try:
                    chunk = os.read(master, 65536)
                except OSError as error:
                    if error.errno == errno.EIO:
                        break
                    raise
                raw.extend(chunk)
                stream.feed(chunk)
                if b"\x1b[6n" in chunk:
                    # pyte answers no requests itself. A session asks for the
                    # cursor position and then for device attributes, whose
                    # reply ends its startup probe.
                    reply = f"\x1b[{screen.cursor.y + 1};{screen.cursor.x + 1}R"
                    if b"\x1b[c" in chunk:
                        reply += "\x1b[?62c"
                    os.write(master, reply.encode())
            if len(raw) > after and expected in "\n".join(screen.display):
                return
            if process.poll() is not None:
                break
        raise AssertionError(f"{name}: missing {expected!r}\n" + "\n".join(screen.display))

    def finish(fullscreen):
        """Quit with Escape and check that the terminal is as it was found."""
        os.write(master, b"\x1b")
        assert process.wait(timeout=10) == 0
        while select.select([master], [], [], 0.05)[0]:
            chunk = os.read(master, 65536)
            raw.extend(chunk)
        restored = termios.tcgetattr(slave)
        assert restored == original, f"{name}: terminal attributes were not restored"
        assert (b"\x1b[?1049l" in raw) == fullscreen, "alternate screen use is wrong"
        assert b"\x1b[?25h" in raw, "cursor was not restored"
        assert b"\x1b[?2026h" in raw, "frames were not synchronized"
        print(f"{name}: input and terminal restoration passed")

    def check_border():
        if name == "gallery":
            for row in screen.display[3:-2]:
                assert row[18] == "│" and row[-1] == "│", f"clipped panel border: {row!r}"

    try:
        receive("Wove")
        for index, (keys, expected) in enumerate(steps):
            os.write(master, keys)
            receive(expected)
            check_border()
            (OUT / f"{name}-{index}.txt").write_text("\n".join(screen.display) + "\n")
        if not fullscreen:
            rows = [row.strip() for row in screen.display]
            expected = [step[1] for step in steps]
            assert [row for row in rows if row in expected] == expected, rows
            return finish(fullscreen)
        before_resize = len(raw)
        screen.resize(lines=12, columns=40)
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 12, 40, 0, 0))
        os.kill(process.pid, signal.SIGWINCH)
        # Observe the resize frame before sending a separate keyboard event.
        receive("Wove", after=before_resize)
        # An additional key produces an observable update after the resize.
        os.write(master, b"+" if name == "counter" else b"x" if name == "editor" else b"\x01\x7f")
        receive("Count: 3" if name == "counter" else "bytes" if name == "editor" else "Text")
        check_border()
        (OUT / f"{name}-resized.txt").write_text("\n".join(screen.display) + "\n")
        finish(fullscreen)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait()
        (OUT / f"{name}.ansi").write_bytes(raw)
        os.close(master)
        os.close(slave)


scenarios = {
    "counter": [(b"++", "Count: 2")],
    "gallery": [(b"\x1b[200~scroll\x1b[201~", "A clipped viewport.")],
    "editor": [(b"\x1b[200~\nNew line\x1b[201~", "New line")],
    "inline": [(b"one\r", "one"), (b"two\r", "two")],
}
# Package CI selects its own examples; a local invocation without names runs all.
selected = sys.argv[1:] or list(scenarios)
if unknown := set(selected) - scenarios.keys():
    raise SystemExit(f"Unknown scenarios: {', '.join(sorted(unknown))}")
for name in selected:
    scenario(name, scenarios[name], fullscreen=name != "inline")
