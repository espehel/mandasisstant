//! Push-to-talk speech recognition: `start_listening` records the default microphone with cpal,
//! and `stop_listening` decodes the whole recording once with the bilingual X-ASR model.

use std::path::Path;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use tauri::async_runtime::spawn_blocking;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, State};

/// Model directory inside the app resources, downloaded by `scripts/fetch-models.sh`.
const MODEL_DIR: &str = "models/sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03";
const ENCODER: &str = "encoder-epoch-99-avg-1.int8.onnx";
/// Upstream ships an fp32 decoder in the int8 package on purpose.
const DECODER: &str = "decoder-epoch-99-avg-1.onnx";
const JOINER: &str = "joiner-epoch-99-avg-1.int8.onnx";
const TOKENS: &str = "tokens.txt";
/// Tune with the decode time that `transcribe` logs.
const NUM_THREADS: i32 = 2;

#[derive(Default)]
pub struct Speech {
    /// Loaded on first use and reused by every recording.
    recognizer: OnceLock<Arc<OfflineRecognizer>>,
    recording: Mutex<Option<Recording>>,
}

impl Speech {
    /// Returns the recognizer, loading the model on first use.
    async fn recognizer(&self, app: &AppHandle) -> Result<Arc<OfflineRecognizer>, String> {
        if let Some(recognizer) = self.recognizer.get() {
            return Ok(Arc::clone(recognizer));
        }
        let model_dir = app
            .path()
            .resolve(MODEL_DIR, BaseDirectory::Resource)
            .map_err(|e| format!("Couldn't locate the speech model: {e}"))?;
        let recognizer = spawn_blocking(move || load_recognizer(&model_dir))
            .await
            .map_err(|e| e.to_string())??;
        // If two first calls raced, both loaded the model; keep the one stored first.
        Ok(Arc::clone(
            self.recognizer.get_or_init(|| Arc::new(recognizer)),
        ))
    }
}

#[tauri::command]
pub async fn start_listening(app: AppHandle, speech: State<'_, Speech>) -> Result<(), String> {
    speech.recognizer(&app).await?;

    let mut recording = speech.recording.lock().unwrap();
    if recording.is_some() {
        return Err("Already recording.".into());
    }
    *recording = Some(Recording::start()?);
    Ok(())
}

#[tauri::command]
pub async fn stop_listening(app: AppHandle, speech: State<'_, Speech>) -> Result<String, String> {
    let recording = speech
        .recording
        .lock()
        .unwrap()
        .take()
        .ok_or("Not recording.")?;
    let (sample_rate, samples) = recording.finish()?;
    let recognizer = speech.recognizer(&app).await?;
    spawn_blocking(move || transcribe(&recognizer, sample_rate, &samples))
        .await
        .map_err(|e| e.to_string())?
}

/// An open microphone stream and the mono chunks it has captured.
struct Recording {
    /// Capture runs until the stream is dropped. Declared before `chunks` so it drops first.
    stream: cpal::Stream,
    sample_rate: i32,
    chunks: Receiver<Vec<f32>>,
}

impl Recording {
    /// Starts capturing the default input device at its native config, downmixed to mono.
    fn start() -> Result<Self, String> {
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
                    // The receiver outlives the stream, so this can't fail.
                    let _ = sender.send(mono);
                },
                |error| eprintln!("Microphone stream error: {error}"),
                None,
            )
            .map_err(|e| format!("Couldn't open the microphone: {e}"))?;
        stream
            .play()
            .map_err(|e| format!("Couldn't start the microphone: {e}"))?;

        Ok(Self {
            stream,
            sample_rate: config.sample_rate() as i32,
            chunks,
        })
    }

    /// Stops capture and returns the sample rate and every captured sample.
    fn finish(self) -> Result<(i32, Vec<f32>), String> {
        // Dropping the stream stops the callback, so this drains everything it sent.
        drop(self.stream);
        let samples: Vec<f32> = self.chunks.try_iter().flatten().collect();

        // CoreAudio doesn't report a denied microphone; it delivers silence instead.
        if samples.iter().all(|&sample| sample == 0.0) {
            return Err(
                "No audio was captured. Allow microphone access in System Settings → \
                        Privacy & Security → Microphone, then try again."
                    .into(),
            );
        }
        Ok((self.sample_rate, samples))
    }
}

/// Loads the X-ASR model, naming any missing file.
fn load_recognizer(model_dir: &Path) -> Result<OfflineRecognizer, String> {
    let model_file = |name: &str| {
        let path = model_dir.join(name);
        if path.is_file() {
            Ok(path.to_string_lossy().into_owned())
        } else {
            Err(format!("Speech model file is missing: {}", path.display()))
        }
    };

    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig {
        encoder: Some(model_file(ENCODER)?),
        decoder: Some(model_file(DECODER)?),
        joiner: Some(model_file(JOINER)?),
    };
    config.model_config.tokens = Some(model_file(TOKENS)?);
    config.model_config.num_threads = NUM_THREADS;

    let started = Instant::now();
    let recognizer = OfflineRecognizer::create(&config).ok_or_else(|| {
        format!(
            "Couldn't load the speech model from {}",
            model_dir.display()
        )
    })?;
    eprintln!("Loaded the speech model in {:.2?}", started.elapsed());
    Ok(recognizer)
}

/// Decodes one complete recording.
fn transcribe(
    recognizer: &OfflineRecognizer,
    sample_rate: i32,
    samples: &[f32],
) -> Result<String, String> {
    let started = Instant::now();
    // A fresh stream per recording: sherpa-onnx exits the process if one stream receives two
    // sample rates. The recognizer resamples to the model's 16 kHz itself.
    let stream = recognizer.create_stream();
    stream.accept_waveform(sample_rate, samples);
    recognizer.decode(&stream);
    let result = stream
        .get_result()
        .ok_or("Speech recognition returned no result.")?;
    eprintln!(
        "Transcribed {:.1} s of audio in {:.2?}",
        samples.len() as f32 / sample_rate as f32,
        started.elapsed()
    );
    Ok(result.text.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transcribes_mixed_language_speech_with_punctuation() {
        let model_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join(MODEL_DIR);
        let recognizer = load_recognizer(&model_dir).unwrap();
        let wav = model_dir.join("test_wavs/0.wav");
        let wave = sherpa_onnx::Wave::read(wav.to_str().unwrap()).unwrap();

        let text = transcribe(&recognizer, wave.sample_rate(), wave.samples()).unwrap();

        assert_eq!(
            text,
            "昨天是 Monday, today is 礼拜二， the day after tomorrow 是星期三。"
        );
    }

    #[test]
    fn names_the_missing_model_file() {
        let error = load_recognizer(Path::new("/nonexistent")).err().unwrap();

        assert_eq!(
            error,
            format!("Speech model file is missing: /nonexistent/{ENCODER}")
        );
    }
}
