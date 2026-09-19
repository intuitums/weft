//! Inline rendering checked against what a terminal would show and keep in
//! its scrollback, not against escape bytes.
use wove::{Buffer, Depth, Inline, Style};

/// Just enough of a terminal for the sequences the renderer writes: absolute
/// positioning, carriage return, line feed with scrolling, and erasing.
struct Vt {
    width: usize,
    rows: Vec<Vec<char>>,
    scrollback: Vec<String>,
    cursor: (usize, usize),
    bytes: Vec<u8>,
}
impl Vt {
    fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            rows: vec![vec![' '; width]; height],
            scrollback: Vec::new(),
            cursor: (0, 0),
            bytes: Vec::new(),
        }
    }
    fn screen(&self) -> Vec<String> {
        let line = |row: &Vec<char>| row.iter().collect::<String>().trim_end().to_owned();
        self.rows.iter().map(line).collect()
    }
    /// Everything the user can scroll back through, then the screen.
    fn history(&self) -> Vec<String> {
        let mut all = self.scrollback.clone();
        all.extend(self.screen());
        while all.last().is_some_and(String::is_empty) {
            all.pop();
        }
        all
    }
    fn feed(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
        let text = String::from_utf8(bytes.to_vec()).unwrap();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '\r' => self.cursor.0 = 0,
                '\n' if self.cursor.1 + 1 == self.rows.len() => {
                    let gone = self.rows.remove(0);
                    self.scrollback
                        .push(gone.iter().collect::<String>().trim_end().to_owned());
                    self.rows.push(vec![' '; self.width]);
                }
                '\n' => self.cursor.1 += 1,
                '\x1b' if chars.next_if_eq(&']').is_some() => {
                    while chars.next().is_some_and(|c| c != '\\') {}
                }
                '\x1b' => {
                    assert_eq!(chars.next(), Some('['));
                    let mut body = String::new();
                    let end = loop {
                        match chars.next().unwrap() {
                            c @ ('@'..='~') => break c,
                            c => body.push(c),
                        }
                    };
                    match (end, body.as_str()) {
                        ('H', "") => self.cursor = (0, 0),
                        ('H', at) => {
                            let (row, column) = at.split_once(';').unwrap();
                            self.cursor = (
                                column.parse::<usize>().unwrap() - 1,
                                row.parse::<usize>().unwrap() - 1,
                            );
                        }
                        ('J', "2") => self.rows.fill(vec![' '; self.width]),
                        ('J', other) => panic!("scrollback must survive: ESC[{other}J"),
                        ('K', _) => self.rows[self.cursor.1][self.cursor.0..].fill(' '),
                        _ => {}
                    }
                }
                c => {
                    self.rows[self.cursor.1][self.cursor.0] = c;
                    self.cursor.0 = (self.cursor.0 + 1).min(self.width - 1);
                }
            }
        }
    }
}

fn frame(rows: &[&str]) -> Buffer {
    let mut buffer = Buffer::new(8, rows.len() as u16);
    for (y, row) in rows.iter().enumerate() {
        let area = wove::Rect::new(0, y as u16, 8, 1);
        buffer.write(area, row, Style::default());
    }
    buffer
}

fn draw(inline: &mut Inline, vt: &mut Vt, rows: &[&str]) {
    let mut bytes = Vec::new();
    inline
        .draw(&mut bytes, &frame(rows), vt.rows.len() as u16)
        .unwrap();
    vt.feed(&bytes);
}

#[test]
fn a_frame_starts_at_the_launch_row_and_leaves_the_shell_above_it() {
    let mut vt = Vt::new(8, 4);
    vt.feed(b"$ app\r\n");
    let mut inline = Inline::new(1, Depth::Rgb);
    draw(&mut inline, &mut vt, &["one", "two"]);
    assert_eq!(vt.screen(), ["$ app", "one", "two", ""]);
    let before = vt.bytes.len();
    draw(&mut inline, &mut vt, &["one", "two"]);
    assert_eq!(vt.bytes.len(), before, "an unchanged frame writes nothing");
}

#[test]
fn growth_past_the_bottom_reaches_scrollback_in_order_with_final_content() {
    let mut vt = Vt::new(8, 3);
    let mut inline = Inline::new(0, Depth::Rgb);
    draw(&mut inline, &mut vt, &["a", "b"]);
    // Row b changes in the same frame that pushes it off the screen.
    draw(&mut inline, &mut vt, &["a", "B", "c", "d", "e", "f"]);
    assert_eq!(vt.history(), ["a", "B", "c", "d", "e", "f"]);
    assert_eq!(vt.screen(), ["d", "e", "f"]);
    // Rows that left the screen belong to the terminal and are never rewritten.
    draw(&mut inline, &mut vt, &["A", "B", "c", "d", "e", "F"]);
    assert_eq!(vt.history(), ["a", "B", "c", "d", "e", "F"]);
}

#[test]
fn small_growth_scrolls_and_rewrites_only_what_changed() {
    let mut vt = Vt::new(8, 3);
    let mut inline = Inline::new(0, Depth::Rgb);
    draw(&mut inline, &mut vt, &["a", "b", "c"]);
    draw(&mut inline, &mut vt, &["a", "b", "c", "d"]);
    assert_eq!(vt.history(), ["a", "b", "c", "d"]);
}

#[test]
fn committed_rows_stay_put_while_later_frames_start_beneath_them() {
    let mut vt = Vt::new(8, 4);
    let mut inline = Inline::new(0, Depth::Rgb);
    draw(&mut inline, &mut vt, &["done", "done 2", "live"]);
    inline.commit(2);
    draw(&mut inline, &mut vt, &["live!", "dock"]);
    assert_eq!(vt.screen(), ["done", "done 2", "live!", "dock"]);
    draw(&mut inline, &mut vt, &["dock"]);
    assert_eq!(vt.screen(), ["done", "done 2", "dock", ""]);
}

#[test]
fn a_shrinking_full_screen_frame_keeps_its_last_row_at_the_bottom() {
    let mut vt = Vt::new(8, 3);
    let mut inline = Inline::new(0, Depth::Rgb);
    draw(&mut inline, &mut vt, &["a", "b", "c", "d", "dock"]);
    draw(&mut inline, &mut vt, &["a", "b", "c", "dock"]);
    assert_eq!(vt.screen(), ["b", "c", "dock"]);
}

#[test]
fn a_resize_repaints_the_visible_tail_without_touching_scrollback() {
    let mut vt = Vt::new(8, 3);
    let mut inline = Inline::new(0, Depth::Rgb);
    draw(&mut inline, &mut vt, &["a", "b", "c", "d"]);
    vt.rows.pop();
    draw(&mut inline, &mut vt, &["a", "b", "c", "d"]);
    assert_eq!(vt.scrollback, ["a"]);
    assert_eq!(vt.screen(), ["c", "d"]);
}

#[test]
fn a_screen_row_maps_onto_the_frame_wherever_it_has_scrolled_to() {
    let mut vt = Vt::new(8, 3);
    vt.feed(b"$ app\r\n");
    let mut inline = Inline::new(1, Depth::Rgb);
    draw(&mut inline, &mut vt, &["a", "b"]);
    assert_eq!(
        inline.frame_row(0),
        None,
        "the shell's row is not the frame's"
    );
    assert_eq!(inline.frame_row(1), Some(0));
    draw(&mut inline, &mut vt, &["a", "b", "c", "d"]);
    assert_eq!(vt.screen(), ["b", "c", "d"]);
    assert_eq!(inline.frame_row(0), Some(1));
}

#[test]
fn finishing_parks_the_cursor_on_a_fresh_line_below_the_frame() {
    let mut vt = Vt::new(8, 3);
    let mut inline = Inline::new(0, Depth::Rgb);
    draw(&mut inline, &mut vt, &["a", "b", "c"]);
    let mut bytes = Vec::new();
    inline.finish(&mut bytes).unwrap();
    vt.feed(&bytes);
    vt.feed(b"$ ");
    assert_eq!(vt.history(), ["a", "b", "c", "$"]);
}
