//! Library logic: search, smart playlists, the play queue, history, filename tags, duplicate
//! detection and list reordering. Pure functions over the show model; no I/O.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod filename;
pub mod history;
pub mod order;
pub mod queue;
pub mod search;
pub mod smart;

pub use queue::Queue;
pub use search::Query;
