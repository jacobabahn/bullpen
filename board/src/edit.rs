//! The line you type a tell or an answer on: the text, the cursor in it, and the
//! editing keys. The picker and the tour stepper (`LineEdit` in cli/src/line_edit.rs) take
//! the same keys.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::cell::Cell;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LineEdit {
    chars: Vec<char>,
    /// the cursor: how many characters are before it
    at: usize,
    /// the first character shown when the line is longer than its room (moved as it's drawn)
    from: Cell<usize>,
}

impl From<&str> for LineEdit {
    fn from(text: &str) -> LineEdit {
        let mut e = LineEdit::default();
        e.paste(text);
        e
    }
}

impl LineEdit {
    pub fn text(&self) -> String {
        self.chars.iter().collect()
    }

    /// Apply an editing key. False when it isn't one (enter, esc), which the caller handles.
    pub fn key(&mut self, k: KeyEvent) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let alt = k.modifiers.contains(KeyModifiers::ALT);
        let word = ctrl || alt;
        match k.code {
            KeyCode::Left if word => self.at = self.word_start(),
            KeyCode::Right if word => self.at = self.word_end(),
            KeyCode::Left => self.at = self.at.saturating_sub(1),
            KeyCode::Right => self.at = (self.at + 1).min(self.chars.len()),
            KeyCode::Home => self.at = 0,
            KeyCode::End => self.at = self.chars.len(),
            KeyCode::Backspace if word => self.cut(self.word_start(), self.at),
            KeyCode::Backspace => self.cut(self.at.saturating_sub(1), self.at),
            KeyCode::Delete => self.cut(self.at, (self.at + 1).min(self.chars.len())),
            KeyCode::Char(c) if ctrl => match c {
                'a' => self.at = 0,
                'e' => self.at = self.chars.len(),
                'b' => self.at = self.at.saturating_sub(1),
                'f' => self.at = (self.at + 1).min(self.chars.len()),
                'h' => self.cut(self.at.saturating_sub(1), self.at),
                'w' => self.cut(self.word_start(), self.at),
                'u' => self.cut(0, self.at),
                'k' => self.cut(self.at, self.chars.len()),
                _ => return false,
            },
            KeyCode::Char(c) if alt => match c {
                'b' => self.at = self.word_start(),
                'f' => self.at = self.word_end(),
                'd' => self.cut(self.at, self.word_end()),
                _ => return false,
            },
            KeyCode::Char(c) => self.insert(&[c]),
            _ => return false,
        }
        true
    }

    /// Pasted text, at the cursor: on one line, so a newline or a tab is a space
    /// (a pasted newline doesn't send it), and other control characters go.
    pub fn paste(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n");
        let chars: Vec<char> = text
            .chars()
            .filter_map(|c| match c {
                '\n' | '\r' | '\t' => Some(' '),
                c if c.is_control() => None,
                c => Some(c),
            })
            .collect();
        self.insert(&chars);
    }

    fn insert(&mut self, cs: &[char]) {
        self.chars.splice(self.at..self.at, cs.iter().copied());
        self.at += cs.len();
    }

    fn cut(&mut self, from: usize, to: usize) {
        self.chars.drain(from..to);
        self.at = from;
    }

    /// Where the word before the cursor starts (the spaces after it go with it).
    fn word_start(&self) -> usize {
        let mut i = self.at;
        while i > 0 && self.chars[i - 1].is_whitespace() {
            i -= 1;
        }
        while i > 0 && !self.chars[i - 1].is_whitespace() {
            i -= 1;
        }
        i
    }

    /// Where the word after the cursor ends.
    fn word_end(&self) -> usize {
        let n = self.chars.len();
        let mut i = self.at;
        while i < n && self.chars[i].is_whitespace() {
            i += 1;
        }
        while i < n && !self.chars[i].is_whitespace() {
            i += 1;
        }
        i
    }

    /// What's shown in `room` columns, and the cursor's column in it. The shown part
    /// moves only when the cursor would leave it, as a shell's line does.
    pub fn view(&self, room: usize) -> (String, usize) {
        let w = |cs: &[char]| cs.iter().map(|c| c.width().unwrap_or(0)).sum::<usize>();
        // the cursor takes a column of its own at the end
        let room = room.max(1);
        let mut from = self.from.get().min(self.at);
        while from < self.at && w(&self.chars[from..self.at]) >= room {
            from += 1;
        }
        self.from.set(from);
        let mut to = from;
        while to < self.chars.len() && w(&self.chars[from..=to]) <= room {
            to += 1;
        }
        let shown: String = self.chars[from..to].iter().collect();
        (shown, w(&self.chars[from..self.at]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(e: &mut LineEdit, code: KeyCode, m: KeyModifiers) -> bool {
        e.key(KeyEvent::new(code, m))
    }

    fn typed(e: &mut LineEdit, s: &str) {
        for c in s.chars() {
            press(e, KeyCode::Char(c), KeyModifiers::NONE);
        }
    }

    /// The text with a | at the cursor.
    fn shown(e: &LineEdit) -> String {
        let mut s = e.chars.clone();
        s.insert(e.at, '|');
        s.into_iter().collect()
    }

    const NONE: KeyModifiers = KeyModifiers::NONE;
    const CTRL: KeyModifiers = KeyModifiers::CONTROL;
    const ALT: KeyModifiers = KeyModifiers::ALT;

    #[test]
    fn left_and_right_move_and_typing_goes_in_at_the_cursor() {
        let mut e = LineEdit::default();
        typed(&mut e, "use postgres");
        press(&mut e, KeyCode::Left, NONE);
        press(&mut e, KeyCode::Left, NONE);
        typed(&mut e, "X");
        assert_eq!(shown(&e), "use postgrX|es");
        press(&mut e, KeyCode::Home, NONE);
        typed(&mut e, "> ");
        assert_eq!(shown(&e), "> |use postgrXes");
        press(&mut e, KeyCode::Char('e'), CTRL);
        assert_eq!(shown(&e), "> use postgrXes|");
        press(&mut e, KeyCode::Char('a'), CTRL);
        press(&mut e, KeyCode::Right, NONE);
        press(&mut e, KeyCode::Delete, NONE);
        assert_eq!(shown(&e), ">|use postgrXes");
        press(&mut e, KeyCode::End, NONE);
        press(&mut e, KeyCode::Right, NONE); // stops at the end
        press(&mut e, KeyCode::Backspace, NONE);
        assert_eq!(shown(&e), ">use postgrXe|");
    }

    #[test]
    fn ctrl_w_ctrl_u_and_ctrl_k_cut_from_the_cursor() {
        let mut e = LineEdit::default();
        typed(&mut e, "keep the old table  ");
        press(&mut e, KeyCode::Char('w'), CTRL);
        assert_eq!(shown(&e), "keep the old |");
        press(&mut e, KeyCode::Left, CTRL); // a word back
        press(&mut e, KeyCode::Char('w'), CTRL);
        assert_eq!(shown(&e), "keep |old ");
        press(&mut e, KeyCode::Char('k'), CTRL);
        assert_eq!(shown(&e), "keep |");
        press(&mut e, KeyCode::Char('b'), ALT);
        press(&mut e, KeyCode::Char('f'), ALT);
        assert_eq!(shown(&e), "keep| ");
        press(&mut e, KeyCode::Char('u'), CTRL);
        assert_eq!(shown(&e), "| ");
        press(&mut e, KeyCode::Backspace, ALT);
        assert_eq!(shown(&e), "| ");
    }

    #[test]
    fn other_ctrl_keys_and_enter_arent_editing_keys() {
        let mut e = LineEdit::default();
        for (code, m) in [(KeyCode::Char('c'), CTRL), (KeyCode::Char('x'), CTRL), (KeyCode::Enter, NONE), (KeyCode::Esc, NONE)] {
            assert!(!press(&mut e, code, m), "{code:?}");
        }
        assert_eq!(e.text(), "");
    }

    #[test]
    fn a_paste_is_one_line_and_has_no_cap() {
        let mut e = LineEdit::default();
        typed(&mut e, "[]");
        press(&mut e, KeyCode::Left, NONE);
        e.paste("one\r\ntwo\nthree\tfour\x07");
        assert_eq!(shown(&e), "[one two three four|]");
        let long = "x".repeat(3000);
        e.paste(&long);
        assert_eq!(e.text().len(), 3000 + "[one two three four]".len());
    }

    #[test]
    fn the_view_keeps_the_cursor_in_it() {
        let mut e = LineEdit::from("abcdefghij");
        // at the end: the end shows, with a column for the cursor
        assert_eq!(e.view(5), ("ghij".into(), 4));
        // moving left keeps the view until the cursor reaches its left edge
        for _ in 0..3 {
            press(&mut e, KeyCode::Left, NONE);
        }
        assert_eq!(e.view(5), ("ghij".into(), 1));
        press(&mut e, KeyCode::Left, NONE);
        press(&mut e, KeyCode::Left, NONE);
        assert_eq!(e.view(5), ("fghij".into(), 0));
        press(&mut e, KeyCode::Home, NONE);
        assert_eq!(e.view(5), ("abcde".into(), 0));
        // a short line shows whole
        assert_eq!(LineEdit::from("hi").view(10), ("hi".into(), 2));
    }
}
