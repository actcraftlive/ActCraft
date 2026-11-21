//! Regions on the playback timeline: loop, skip and stop after.
//!
//! The table is a fixed-size array so the renderer can hold and swap it without allocating.

/// Regions a song may have.
pub const MAX_REGIONS: usize = 64;

/// What playback does at a region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RegionAction {
    /// Plays through (a named part or a cue).
    #[default]
    Play,
    /// Jumps back to the start at the end, until told to leave.
    Loop,
    /// Jumped over; consecutive skip regions are skipped as one.
    Skip,
    /// Playback stops at the end.
    StopAfter,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RegionSpan {
    pub start: f64,
    pub end: f64,
    pub action: RegionAction,
}

/// What playback has to do after moving from one time to the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Jump {
    /// Continue at this time (a skip, or back to a loop's start).
    To(f64),
    /// Stop here.
    Stop(f64),
}

/// Up to [`MAX_REGIONS`] regions, sorted by start.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionTable {
    spans: [RegionSpan; MAX_REGIONS],
    len: usize,
}

impl Default for RegionTable {
    fn default() -> Self {
        Self { spans: [RegionSpan::default(); MAX_REGIONS], len: 0 }
    }
}

/// Gap tolerated between skip regions that still count as consecutive (seconds).
const JOIN: f64 = 0.001;

impl RegionTable {
    /// Builds a table; regions with an empty or invalid range are dropped, the rest sorted.
    pub fn new(list: impl IntoIterator<Item = RegionSpan>) -> Self {
        let mut t = Self::default();
        for r in list {
            if t.len >= MAX_REGIONS {
                break;
            }
            if r.start.is_finite() && r.end.is_finite() && r.start >= 0.0 && r.end > r.start {
                if let Some(slot) = t.spans.get_mut(t.len) {
                    *slot = r;
                }
                t.len += 1;
            }
        }
        if let Some(s) = t.spans.get_mut(..t.len) {
            s.sort_by(|a, b| a.start.total_cmp(&b.start));
        }
        t
    }

    pub fn spans(&self) -> &[RegionSpan] {
        self.spans.get(..self.len).unwrap_or(&[])
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Where a run of consecutive skip regions starting with `first` ends.
    fn skip_run_end(&self, first: &RegionSpan) -> f64 {
        let mut end = first.end;
        // Bounded: each pass can only extend the run by one of at most 64 regions.
        for _ in 0..MAX_REGIONS {
            let next = self
                .spans()
                .iter()
                .filter(|r| r.action == RegionAction::Skip && r.start <= end + JOIN && r.end > end)
                .map(|r| r.end)
                .fold(None, |m: Option<f64>, e| Some(m.map_or(e, |m| m.max(e))));
            match next {
                Some(e) => end = e,
                None => break,
            }
        }
        end
    }

    /// What to do having played from `prev` to `t`. `leave_loop` lets a loop play out once.
    pub fn step(&self, prev: f64, t: f64, leave_loop: bool) -> Option<Jump> {
        if self.len == 0 {
            return None;
        }
        // A region ending here acts first: a loop that ends where a skip region begins loops.
        for r in self.spans() {
            if prev < r.end && r.end <= t {
                match r.action {
                    RegionAction::Loop if !leave_loop => return Some(Jump::To(r.start)),
                    RegionAction::StopAfter => return Some(Jump::Stop(r.end)),
                    _ => {}
                }
            }
        }
        for r in self.spans() {
            if r.action == RegionAction::Skip && r.start <= t && t < r.end {
                return Some(Jump::To(self.skip_run_end(r)));
            }
        }
        None
    }

    /// The loop region playing at `t`, if any.
    pub fn loop_at(&self, t: f64) -> Option<RegionSpan> {
        self.spans().iter().copied().find(|r| r.action == RegionAction::Loop && r.start <= t && t < r.end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(start: f64, end: f64, action: RegionAction) -> RegionSpan {
        RegionSpan { start, end, action }
    }

    #[test]
    fn consecutive_skips_are_skipped_as_one() {
        let t = RegionTable::new([
            span(10.0, 12.0, RegionAction::Skip),
            span(12.0, 15.0, RegionAction::Skip),
            span(20.0, 22.0, RegionAction::Skip),
        ]);
        assert_eq!(t.step(9.99, 10.0, false), Some(Jump::To(15.0)));
        assert_eq!(t.step(19.9, 20.5, false), Some(Jump::To(22.0)));
        assert_eq!(t.step(15.0, 15.1, false), None);
    }

    #[test]
    fn loops_repeat_until_left_and_stop_after_stops() {
        let t = RegionTable::new([span(4.0, 8.0, RegionAction::Loop), span(30.0, 32.0, RegionAction::StopAfter)]);
        assert_eq!(t.step(7.99, 8.0, false), Some(Jump::To(4.0)));
        assert_eq!(t.step(7.99, 8.0, true), None);
        assert_eq!(t.step(31.99, 32.01, false), Some(Jump::Stop(32.0)));
        assert!(t.loop_at(5.0).is_some() && t.loop_at(9.0).is_none());
    }

    #[test]
    fn hostile_ranges_are_dropped() {
        let t = RegionTable::new([
            span(f64::NAN, 1.0, RegionAction::Loop),
            span(5.0, 2.0, RegionAction::Skip),
            span(-1.0, 3.0, RegionAction::Skip),
        ]);
        assert!(t.is_empty());
        let many = RegionTable::new((0..500).map(|i| span(f64::from(i), f64::from(i) + 0.5, RegionAction::Play)));
        assert_eq!(many.spans().len(), MAX_REGIONS);
    }
}
