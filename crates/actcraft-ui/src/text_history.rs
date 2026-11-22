//! Undo and redo for a text being edited: every step the person can take back, whether it was
//! typed or made by a button (a time stamp, formatting, a replacement). Typing in one go is one
//! step; each button press is a step of its own.

/// A text and where the cursor was.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Snapshot {
    pub text: String,
    /// Cursor or selection as character indexes (start, end).
    pub selection: (usize, usize),
}

/// How a change was made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Change {
    /// Typed into the text box: joins the step before when it follows within a moment.
    Typing,
    /// A button or command: always a step of its own.
    Action,
}

/// Steps kept to go back.
const MAX_STEPS: usize = 200;
/// Bytes of text kept over all steps; the oldest go first.
const MAX_BYTES: usize = 16 * 1024 * 1024;
/// Typing that pauses longer than this starts a new step, in seconds.
const TYPING_PAUSE: f64 = 1.5;

/// The steps behind and ahead of the text as it is.
#[derive(Debug, Default)]
pub(crate) struct TextHistory {
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// When the last typing was recorded, while typing goes on.
    typing_at: Option<f64>,
}

impl TextHistory {
    /// Records `before`, the state just before a change made at `now` (seconds).
    pub fn record(&mut self, before: Snapshot, change: Change, now: f64) {
        let joins = change == Change::Typing && self.typing_at.is_some_and(|t| now - t <= TYPING_PAUSE && now >= t);
        self.typing_at = (change == Change::Typing).then_some(now);
        self.redo.clear();
        if joins {
            return;
        }
        self.undo.push(before);
        self.trim();
    }

    /// The state to go back to from `current`, which becomes a step to redo.
    pub fn undo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let back = self.undo.pop()?;
        self.redo.push(current);
        self.typing_at = None;
        Some(back)
    }

    /// The state to go forward to from `current`, which becomes a step to undo again.
    pub fn redo(&mut self, current: Snapshot) -> Option<Snapshot> {
        let ahead = self.redo.pop()?;
        self.undo.push(current);
        self.typing_at = None;
        self.trim();
        Some(ahead)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Drops the oldest steps beyond the limits.
    fn trim(&mut self) {
        let over = self.undo.len().saturating_sub(MAX_STEPS);
        self.undo.drain(..over);
        let bytes = |v: &[Snapshot]| v.iter().map(|s| s.text.len()).sum::<usize>();
        while self.undo.len() > 1 && bytes(&self.undo) + bytes(&self.redo) > MAX_BYTES {
            self.undo.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(text: &str) -> Snapshot {
        Snapshot { text: text.to_owned(), selection: (text.chars().count(), text.chars().count()) }
    }

    #[test]
    fn typing_in_one_go_is_one_step_and_actions_are_their_own() {
        let mut h = TextHistory::default();
        // Typing "abc" quickly, a pause, then "d".
        h.record(at(""), Change::Typing, 0.0);
        h.record(at("a"), Change::Typing, 0.3);
        h.record(at("ab"), Change::Typing, 0.6);
        h.record(at("abc"), Change::Typing, 5.0);
        // A time stamp, then another.
        h.record(at("abcd"), Change::Action, 5.1);
        h.record(at("[00:01.00]abcd"), Change::Action, 5.2);
        let now = at("[00:02.00]abcd");
        let back = h.undo(now.clone());
        assert_eq!(back, Some(at("[00:01.00]abcd")), "the second stamp goes back alone");
        let back = h.undo(back.unwrap_or_default());
        assert_eq!(back, Some(at("abcd")));
        let back = h.undo(back.unwrap_or_default());
        assert_eq!(back, Some(at("abc")), "typing after a pause is its own step");
        let back = h.undo(back.unwrap_or_default());
        assert_eq!(back, Some(at("")), "quick typing is one step");
        assert!(!h.can_undo());
        // Redo walks forward again to where it was.
        let mut cur = back.unwrap_or_default();
        for _ in 0..4 {
            cur = h.redo(cur).unwrap_or_default();
        }
        assert_eq!(cur, now);
        assert!(!h.can_redo());
    }

    #[test]
    fn a_new_change_drops_what_could_be_redone() {
        let mut h = TextHistory::default();
        h.record(at("a"), Change::Action, 0.0);
        let back = h.undo(at("b")).unwrap_or_default();
        assert!(h.can_redo());
        h.record(back, Change::Action, 1.0);
        assert!(!h.can_redo());
        // Typing right after an undo is a step of its own.
        let mut h = TextHistory::default();
        h.record(at(""), Change::Typing, 0.0);
        let back = h.undo(at("x")).unwrap_or_default();
        h.record(back, Change::Typing, 0.1);
        assert_eq!(h.undo(at("y")), Some(at("")));
    }

    #[test]
    fn steps_and_bytes_are_bounded() {
        let mut h = TextHistory::default();
        for i in 0..(MAX_STEPS + 50) {
            h.record(at(&i.to_string()), Change::Action, i as f64);
        }
        assert_eq!(h.undo.len(), MAX_STEPS);
        assert_eq!(h.undo.first().map(|s| s.text.clone()), Some(50.to_string()), "the oldest went first");
        let big = "x".repeat(MAX_BYTES / 3);
        for i in 0..5 {
            h.record(Snapshot { text: big.clone(), selection: (0, 0) }, Change::Action, 1000.0 + f64::from(i));
        }
        assert!(h.undo.iter().map(|s| s.text.len()).sum::<usize>() <= MAX_BYTES);
        assert!(h.can_undo());
    }
}
