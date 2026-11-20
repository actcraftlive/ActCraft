//! Audio output through the operating system's audio API.
//!
//! The device callback owns the [`Renderer`]; the engine keeps the [`actcraft_audio::Controller`].
//! The callback converts the renderer's `f32` output to the device's sample format through a
//! scratch buffer allocated before the stream starts, so the callback itself never allocates.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use actcraft_audio::{Controller, Renderer, link};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SampleFormat, SizedSample, StreamConfig};

/// Frames converted per scratch pass.
const SCRATCH_FRAMES: usize = 4096;

/// A device that can play audio.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputInfo {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// Output devices of the default audio API.
pub fn output_devices() -> Vec<OutputInfo> {
    let host = cpal::default_host();
    let default_id = host.default_output_device().and_then(|d| d.id().ok()).map(|id| id.to_string());
    let Ok(devices) = host.output_devices() else { return Vec::new() };
    devices
        .take(256)
        .filter_map(|d| {
            let id = d.id().ok()?.to_string();
            let name = d.description().map(|desc| desc.name().to_owned()).unwrap_or_else(|_| id.clone());
            Some(OutputInfo { is_default: default_id.as_deref() == Some(id.as_str()), id, name })
        })
        .collect()
}

/// A running output stream. Dropping it stops the audio.
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub device_name: String,
    pub sample_rate: u32,
    pub channels: u16,
    errors: Arc<AtomicU32>,
}

impl AudioOutput {
    /// Opens the default output device (or the one with `device_id`) and starts rendering.
    /// Returns the controller for the engine.
    pub fn open(device_id: Option<&str>) -> Result<(AudioOutput, Controller), String> {
        let host = cpal::default_host();
        let device = match device_id {
            Some(id) => {
                let parsed = id.parse().map_err(|_| format!("`{id}` is not a device id"))?;
                host.device_by_id(&parsed).ok_or_else(|| format!("no audio device {id}"))?
            }
            None => host.default_output_device().ok_or_else(|| "no audio output device found".to_owned())?,
        };
        let device_name =
            device.description().map(|d| d.name().to_owned()).unwrap_or_else(|_| "Audio output".to_owned());
        let supported = device.default_output_config().map_err(|e| format!("audio device: {e}"))?;
        let format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let (controller, renderer) = link(config.sample_rate);
        let errors = Arc::new(AtomicU32::new(0));
        let stream = match format {
            SampleFormat::F32 => build::<f32>(&device, &config, renderer, Arc::clone(&errors)),
            SampleFormat::I16 => build::<i16>(&device, &config, renderer, Arc::clone(&errors)),
            SampleFormat::I32 => build::<i32>(&device, &config, renderer, Arc::clone(&errors)),
            SampleFormat::U16 => build::<u16>(&device, &config, renderer, Arc::clone(&errors)),
            SampleFormat::F64 => build::<f64>(&device, &config, renderer, Arc::clone(&errors)),
            other => Err(format!("audio format {other} is not supported yet")),
        }?;
        stream.play().map_err(|e| format!("cannot start audio: {e}"))?;
        Ok((
            AudioOutput {
                _stream: stream,
                device_name,
                sample_rate: config.sample_rate,
                channels: config.channels,
                errors,
            },
            controller,
        ))
    }

    /// Stream errors (dropouts, device changes) since the stream started.
    pub fn error_count(&self) -> u32 {
        self.errors.load(Ordering::Relaxed)
    }
}

fn build<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut renderer: Renderer,
    errors: Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels.max(1));
    let mut scratch = vec![0.0f32; SCRATCH_FRAMES * channels];
    let data = move |out: &mut [T], _: &cpal::OutputCallbackInfo| {
        for chunk in out.chunks_mut(SCRATCH_FRAMES * channels) {
            let n = chunk.len();
            if let Some(buf) = scratch.get_mut(..n) {
                renderer.process(buf, channels);
                for (o, s) in chunk.iter_mut().zip(buf.iter()) {
                    *o = T::from_sample(*s);
                }
            }
        }
    };
    let err = move |_e: cpal::Error| {
        errors.fetch_add(1, Ordering::Relaxed);
    };
    device.build_output_stream(*config, data, err, None).map_err(|e| format!("cannot open audio stream: {e}"))
}
