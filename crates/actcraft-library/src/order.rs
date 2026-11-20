//! Reordering lists: multi-select moves and non-destructive sorted views.

use std::cmp::Ordering;

use actcraft_show::Song;

/// Moves the items at `picked` (any order, duplicates ignored) so they sit together, in their
/// original order, before the item that was at `to` (or at the end when `to >= len`). Out-of-range
/// picks are ignored.
pub fn move_items<T: Clone>(list: &[T], picked: &[usize], to: usize) -> Vec<T> {
    let mut chosen: Vec<usize> = picked.iter().copied().filter(|&i| i < list.len()).collect();
    chosen.sort_unstable();
    chosen.dedup();
    let before_target = chosen.iter().filter(|&&i| i < to).count();
    let mut rest: Vec<T> = Vec::with_capacity(list.len());
    let mut moved: Vec<T> = Vec::with_capacity(chosen.len());
    for (i, item) in list.iter().enumerate() {
        if chosen.binary_search(&i).is_ok() {
            moved.push(item.clone());
        } else {
            rest.push(item.clone());
        }
    }
    let at = to.min(list.len()).saturating_sub(before_target).min(rest.len());
    let tail = rest.split_off(at);
    rest.extend(moved);
    rest.extend(tail);
    rest
}

/// Keys a set can be viewed sorted by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Title,
    Artist,
    Key,
    Bpm,
    Year,
    Duration,
}

impl SortKey {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "title" => Some(Self::Title),
            "artist" => Some(Self::Artist),
            "key" => Some(Self::Key),
            "bpm" => Some(Self::Bpm),
            "year" => Some(Self::Year),
            "duration" => Some(Self::Duration),
            _ => None,
        }
    }
}

/// The songs of a set in sorted order, without changing the set. Unknown ids keep their place at
/// the end.
pub fn sorted_view(ids: &[u64], songs: &[Song], key: SortKey, descending: bool) -> Vec<u64> {
    let find = |id: u64| songs.iter().find(|s| s.id == id);
    let mut out: Vec<u64> = ids.to_vec();
    out.sort_by(|&a, &b| {
        let ord = match (find(a), find(b)) {
            (Some(x), Some(y)) => compare(x, y, key),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        };
        if descending { ord.reverse() } else { ord }
    });
    out
}

fn compare(a: &Song, b: &Song, key: SortKey) -> Ordering {
    match key {
        SortKey::Title => a.title.to_lowercase().cmp(&b.title.to_lowercase()),
        SortKey::Artist => a.artist.to_lowercase().cmp(&b.artist.to_lowercase()),
        SortKey::Key => a.key.cmp(&b.key),
        SortKey::Bpm => a.bpm.total_cmp(&b.bpm),
        SortKey::Year => a.year.cmp(&b.year),
        SortKey::Duration => a.duration.total_cmp(&b.duration),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moves_selection_together() {
        let l = vec!['a', 'b', 'c', 'd', 'e'];
        assert_eq!(move_items(&l, &[3, 1], 0), vec!['b', 'd', 'a', 'c', 'e']);
        assert_eq!(move_items(&l, &[0, 1], 4), vec!['c', 'd', 'a', 'b', 'e']);
        assert_eq!(move_items(&l, &[0, 99], 999), vec!['b', 'c', 'd', 'e', 'a']);
    }
}
