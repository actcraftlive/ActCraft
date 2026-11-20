//! The play queue: what plays after the current song, at most [`MAX_QUEUE`] songs.

use std::collections::VecDeque;

/// Songs the queue holds.
pub const MAX_QUEUE: usize = 128;

/// What a double tap on a song in a list does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DoubleTap {
    /// Load the song now.
    #[default]
    Load,
    /// Put it next in the queue.
    PlayNext,
    /// Add it at the end of the queue.
    AddToQueue,
    /// Nothing.
    Ignore,
}

impl DoubleTap {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "load" => Some(Self::Load),
            "play_next" => Some(Self::PlayNext),
            "add_to_queue" => Some(Self::AddToQueue),
            "ignore" => Some(Self::Ignore),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Queue {
    items: VecDeque<u64>,
}

impl Queue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &u64> {
        self.items.iter()
    }

    /// Adds at the end. Returns false when the queue is full.
    pub fn push(&mut self, song: u64) -> bool {
        if self.items.len() >= MAX_QUEUE {
            return false;
        }
        self.items.push_back(song);
        true
    }

    /// Puts a song first. When full, the last song makes room.
    pub fn play_next(&mut self, song: u64) {
        if self.items.len() >= MAX_QUEUE {
            self.items.pop_back();
        }
        self.items.push_front(song);
    }

    /// Adds songs at the end until the queue is full; returns how many were added.
    pub fn extend(&mut self, songs: impl IntoIterator<Item = u64>) -> usize {
        let mut added = 0;
        for s in songs {
            if !self.push(s) {
                break;
            }
            added += 1;
        }
        added
    }

    /// Replaces the queue with songs from a playlist (at most [`MAX_QUEUE`]).
    pub fn fill(&mut self, songs: impl IntoIterator<Item = u64>) -> usize {
        self.items.clear();
        self.extend(songs)
    }

    pub fn pop(&mut self) -> Option<u64> {
        self.items.pop_front()
    }

    pub fn remove(&mut self, index: usize) -> Option<u64> {
        self.items.remove(index)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Drops every entry of a song (after the song is deleted).
    pub fn forget(&mut self, song: u64) {
        self.items.retain(|&s| s != song);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_at_max_and_play_next_goes_first() {
        let mut q = Queue::new();
        assert_eq!(q.fill(0..500), MAX_QUEUE);
        assert!(!q.push(9999));
        q.play_next(7777);
        assert_eq!(q.len(), MAX_QUEUE);
        assert_eq!(q.pop(), Some(7777));
    }
}
