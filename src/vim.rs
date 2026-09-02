//! Minimal modal editor state: normal-mode motions/edits over a char buffer.
//! Insert mode is handled natively by the textarea; we sync back on Escape.
//! ponytail: char indices, not UTF-16 units. Astral chars (emoji) shift the
//! cursor by one; convert at the textarea boundary if that ever matters.

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Command,
}

#[derive(Debug, PartialEq)]
pub enum Act {
    None,
    Insert,
    Command,
    Write,
    Quit,
    WriteQuit,
    Edit(String),
    Unknown(String),
}

pub struct Ed {
    pub text: Vec<char>,
    pub cur: usize,
    pub mode: Mode,
    pending: Option<char>,
    hist: Vec<(Vec<char>, usize)>,
}

impl Ed {
    pub fn new() -> Self {
        Ed { text: Vec::new(), cur: 0, mode: Mode::Normal, pending: None, hist: Vec::new() }
    }

    pub fn load(&mut self, s: &str) {
        *self = Ed { text: s.chars().collect(), ..Ed::new() };
    }

    pub fn string(&self) -> String {
        self.text.iter().collect()
    }

    /// Called on Escape: take the textarea's text and caret, step left like vim.
    pub fn leave_insert(&mut self, s: &str, caret: usize) {
        self.text = s.chars().collect();
        let caret = caret.min(self.text.len());
        let ls = self.ls(caret);
        self.cur = if caret > ls { caret - 1 } else { caret };
        self.mode = Mode::Normal;
    }

    fn ls(&self, i: usize) -> usize {
        self.text[..i].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1)
    }

    fn le(&self, i: usize) -> usize {
        self.text[i..].iter().position(|&c| c == '\n').map_or(self.text.len(), |p| i + p)
    }

    /// Normal-mode cursor sits on a char, never on '\n' or past the end (unless the line is empty).
    pub fn clamp(&mut self) {
        self.cur = self.cur.min(self.text.len());
        let ls = self.ls(self.cur);
        if self.cur > ls && self.text.get(self.cur).map_or(true, |&c| c == '\n') {
            self.cur -= 1;
        }
    }

    fn snap(&mut self) {
        self.hist.push((self.text.clone(), self.cur));
        if self.hist.len() > 200 {
            self.hist.remove(0);
        }
    }

    fn move_line(&mut self, down: bool) {
        let ls = self.ls(self.cur);
        let col = self.cur - ls;
        let nls = if down {
            let le = self.le(self.cur);
            if le == self.text.len() {
                return;
            }
            le + 1
        } else {
            if ls == 0 {
                return;
            }
            self.ls(ls - 1)
        };
        self.cur = (nls + col).min(self.le(nls));
        self.clamp();
    }

    fn word_fwd(&mut self) {
        let n = self.text.len();
        let ws = |c: char| c.is_whitespace();
        let mut i = self.cur;
        while i < n && !ws(self.text[i]) {
            i += 1;
        }
        while i < n && ws(self.text[i]) {
            i += 1;
        }
        self.cur = i;
        self.clamp();
    }

    fn word_back(&mut self) {
        let ws = |c: char| c.is_whitespace();
        let mut i = self.cur;
        while i > 0 && ws(self.text[i - 1]) {
            i -= 1;
        }
        while i > 0 && !ws(self.text[i - 1]) {
            i -= 1;
        }
        self.cur = i;
    }

    fn delete_line(&mut self) {
        let ls = self.ls(self.cur);
        let le = self.le(self.cur);
        let (from, to) = if le < self.text.len() {
            (ls, le + 1)
        } else {
            (ls.saturating_sub(1).min(ls), le)
        };
        self.text.drain(from..to);
        self.cur = self.ls(from.min(self.text.len()));
        self.clamp();
    }

    fn insert(&mut self) -> Act {
        self.snap();
        self.mode = Mode::Insert;
        Act::Insert
    }

    /// Handle one normal-mode key (as `KeyboardEvent.key`).
    pub fn key(&mut self, k: &str) -> Act {
        if let Some(p) = self.pending.take() {
            match (p, k) {
                ('d', "d") => {
                    self.snap();
                    self.delete_line();
                }
                ('g', "g") => self.cur = 0,
                _ => {}
            }
            return Act::None;
        }
        let (ls, le) = (self.ls(self.cur), self.le(self.cur));
        match k {
            "h" | "ArrowLeft" => self.cur = self.cur.max(ls + 1) - 1,
            "l" | "ArrowRight" => self.cur = (self.cur + 1).min(le.max(ls + 1) - 1),
            "j" | "ArrowDown" => self.move_line(true),
            "k" | "ArrowUp" => self.move_line(false),
            "0" | "Home" => self.cur = ls,
            "$" | "End" => self.cur = le.max(ls + 1) - 1,
            "w" => self.word_fwd(),
            "b" => self.word_back(),
            "G" => {
                self.cur = self.ls(self.text.len());
                self.clamp();
            }
            "x" => {
                if self.text.get(self.cur).is_some_and(|&c| c != '\n') {
                    self.snap();
                    self.text.remove(self.cur);
                    self.clamp();
                }
            }
            "d" | "g" => self.pending = Some(k.chars().next().unwrap()),
            "u" => {
                if let Some((t, c)) = self.hist.pop() {
                    self.text = t;
                    self.cur = c;
                }
            }
            "i" => return self.insert(),
            "a" => {
                if self.text.get(self.cur).is_some_and(|&c| c != '\n') {
                    self.cur += 1;
                }
                return self.insert();
            }
            "A" => {
                self.cur = le;
                return self.insert();
            }
            "I" => {
                self.cur = ls;
                return self.insert();
            }
            "o" => {
                let a = self.insert();
                self.text.insert(le, '\n');
                self.cur = le + 1;
                return a;
            }
            "O" => {
                let a = self.insert();
                self.text.insert(ls, '\n');
                self.cur = ls;
                return a;
            }
            ":" => {
                self.mode = Mode::Command;
                return Act::Command;
            }
            _ => {}
        }
        Act::None
    }

    /// Run an ex command (without the leading ':').
    pub fn exec(&mut self, cmd: &str) -> Act {
        self.mode = Mode::Normal;
        let cmd = cmd.trim();
        match cmd {
            "w" => Act::Write,
            "q" => Act::Quit,
            "wq" | "x" => Act::WriteQuit,
            _ => match cmd.strip_prefix("e ") {
                Some(name) if !name.trim().is_empty() => Act::Edit(name.trim().to_string()),
                _ => Act::Unknown(cmd.to_string()),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(s: &str) -> Ed {
        let mut e = Ed::new();
        e.load(s);
        e
    }

    fn keys(e: &mut Ed, ks: &str) {
        for k in ks.chars() {
            e.key(&k.to_string());
        }
    }

    #[test]
    fn motions() {
        let mut e = ed("ab\ncdef\n\nx");
        keys(&mut e, "lll");
        assert_eq!(e.cur, 1, "l stops at line end");
        keys(&mut e, "j$");
        assert_eq!(e.cur, 6);
        keys(&mut e, "j");
        assert_eq!(e.cur, 8, "empty line keeps cursor at its start");
        keys(&mut e, "k0");
        assert_eq!(e.cur, 3);
        keys(&mut e, "G");
        assert_eq!(e.cur, 9);
        keys(&mut e, "gg");
        assert_eq!(e.cur, 0);
        keys(&mut e, "ww");
        assert_eq!(e.cur, 9);
        keys(&mut e, "b");
        assert_eq!(e.cur, 3);
    }

    #[test]
    fn edits_and_undo() {
        let mut e = ed("one\ntwo\nthree");
        keys(&mut e, "jdd");
        assert_eq!(e.string(), "one\nthree");
        assert_eq!(e.cur, 4);
        keys(&mut e, "dd");
        assert_eq!(e.string(), "one");
        keys(&mut e, "x");
        assert_eq!(e.string(), "ne");
        keys(&mut e, "uuu");
        assert_eq!(e.string(), "one\ntwo\nthree");
        assert_eq!(e.key("o"), Act::Insert);
        assert_eq!(e.string(), "one\ntwo\n\nthree");
        assert_eq!(e.cur, 8);
        e.leave_insert("one\ntwo\nnew\nthree", 11);
        assert_eq!((e.cur, e.mode), (10, Mode::Normal));
    }

    #[test]
    fn commands() {
        let mut e = ed("");
        assert_eq!(e.key(":"), Act::Command);
        assert_eq!(e.exec("e  notes/todo "), Act::Edit("notes/todo".into()));
        assert_eq!(e.exec("wq"), Act::WriteQuit);
        assert_eq!(e.exec("zz"), Act::Unknown("zz".into()));
        assert_eq!(e.mode, Mode::Normal);
    }
}
