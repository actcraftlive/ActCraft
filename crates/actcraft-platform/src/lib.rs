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

pub mod artnet_nodes;
pub mod audio_out;
pub mod dmx_out;
pub mod files;
pub mod midi_in;
pub mod midi_out;
pub mod osc_in;
pub mod serial;
pub mod speech;

pub use actcraft_audio::StatusReader;
pub use artnet_nodes::{ArtNode, discover as discover_artnet_nodes};
pub use audio_out::{AudioOutput, OutputInfo, output_devices};
pub use dmx_out::{DmxFrame, DmxOut};
pub use files::{NativeServices, RootedServices};
pub use midi_in::{MidiIn, MidiQueue, input_ports as midi_input_ports};
pub use midi_out::{MidiTimeOut, output_ports as midi_output_ports};
pub use osc_in::{OscIn, OscQueue};
pub use serial::ports as serial_ports;
