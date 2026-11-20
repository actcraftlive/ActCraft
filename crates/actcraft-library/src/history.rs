//! Played-song history: the last [`MAX_HISTORY`] songs with a "done" marker, and playlists rebuilt
//! from one day of it.

use actcraft_show::HistoryEntry;
use actcraft_show::limits::MAX_HISTORY;
use chrono::{DateTime, NaiveDate};

/// Records a played song; the oldest entries drop off past [`MAX_HISTORY`].
pub fn record(history: &mut Vec<HistoryEntry>, song: u64, played_at: DateTime<chrono::FixedOffset>) {
    history.push(HistoryEntry { song, played_at: played_at.to_rfc3339(), done: false });
    if history.len() > MAX_HISTORY {
        let drop = history.len() - MAX_HISTORY;
        history.drain(..drop);
    }
}

/// Parses `YYYY-MM-DD`.
pub fn parse_date(s: &str) -> Result<NaiveDate, String> {
    NaiveDate::parse_from_str(s.trim(), "%Y-%m-%d").map_err(|_| format!("`{s}` is not a date like 2026-05-31"))
}

/// Songs played on `date` (in the time zone they were recorded in), first play order, no repeats.
pub fn songs_on(history: &[HistoryEntry], date: NaiveDate) -> Vec<u64> {
    let mut out = Vec::new();
    for h in history {
        let Ok(t) = DateTime::parse_from_rfc3339(&h.played_at) else { continue };
        if t.date_naive() == date && !out.contains(&h.song) {
            out.push(h.song);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_last_500_and_filters_by_day() {
        let mut h = Vec::new();
        let day1 = DateTime::parse_from_rfc3339("2026-05-01T20:00:00+02:00");
        let day2 = DateTime::parse_from_rfc3339("2026-05-02T21:00:00+02:00");
        let (Ok(day1), Ok(day2)) = (day1, day2) else { return };
        for i in 0..600 {
            record(&mut h, i, day1);
        }
        record(&mut h, 1, day2);
        record(&mut h, 2, day2);
        record(&mut h, 1, day2);
        assert_eq!(h.len(), MAX_HISTORY);
        let d = parse_date("2026-05-02").unwrap_or_default();
        assert_eq!(songs_on(&h, d), vec![1, 2]);
    }
}
