//! Default microphone capture, downmixed to mono at the device's native sample rate.

use std::sync::mpsc::{self, Receiver};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Starts capturing the default input device at its native config, downmixed to mono. Returns the
/// stream, its sample rate, and the captured chunks; the channel closes when the stream drops.
pub(super) fn open() -> Result<(cpal::Stream, i32, Receiver<Vec<f32>>), String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No microphone found.")?;
    let config = device
        .default_input_config()
        .map_err(|e| format!("Couldn't read the microphone settings: {e}"))?;
    let channels = usize::from(config.channels());
    let (sender, chunks) = mpsc::channel();

    // CoreAudio converts any device's native format to f32.
    let stream = device
        .build_input_stream(
            config.config(),
            move |data: &[f32], _: &cpal::InputCallbackInfo| {
                let mono = data
                    .chunks_exact(channels)
                    .map(|frame| frame.iter().sum::<f32>() / channels as f32)
                    .collect();
                // Fails only if the worker stopped early; `Session::stop_listening` reports why.
                let _ = sender.send(mono);
            },
            |error| eprintln!("Microphone stream error: {error}"),
            None,
        )
        .map_err(|e| format!("Couldn't open the microphone: {e}"))?;
    stream
        .play()
        .map_err(|e| format!("Couldn't start the microphone: {e}"))?;

    Ok((stream, config.sample_rate() as i32, chunks))
}
