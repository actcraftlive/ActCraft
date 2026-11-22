//! Desktop platform services: real files, the local clock, folder-restricted access for automation,
//! and audio output through the operating system (WASAPI, Core Audio, ALSA/PipeWire via cpal).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable
)]

pub mod audio_out;
pub mod files;
pub mod speech;

pub use audio_out::{AudioOutput, OutputInfo, output_devices};
pub use files::{NativeServices, RootedServices};
