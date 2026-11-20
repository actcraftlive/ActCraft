//! The realtime audio side of ActCraft.
//!
//! [`link`] makes a [`Controller`] (used by the engine) and a [`Renderer`] (owned by the audio
//! callback). They talk through lock-free single-producer ring buffers. The renderer never
//! allocates, frees or locks: song data arrives already built in a `Box`, and replaced data is
//! handed back to the controller to be freed outside the callback.
//!
//! Every track's read position is derived from one shared clock on every frame, so tracks with
//! different sample rates can't drift apart over a long song (continuous drift correction).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod clip;
pub mod clock;
pub mod dsp;
pub mod peaks;
pub mod render;
pub mod shared;
pub mod tempo;

pub use clip::Clip;
pub use clock::BeatClock;
pub use render::{
    BUSES, BusParams, ClickMode, Controller, MAX_TRACKS, MetronomeParams, Msg, Renderer, SongLoad, Status, TrackParams,
    link,
};
