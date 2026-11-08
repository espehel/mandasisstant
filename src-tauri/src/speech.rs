//! Listening sessions: `start_listening` opens the default microphone with cpal and starts a
//! worker thread that splits the audio into speech segments with Silero VAD and transcribes each
//! segment with the bilingual X-ASR model as soon as it ends. `stop_listening` closes the
//! microphone and waits for the worker to transcribe any trailing speech.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use sherpa_onnx::{
    LinearResampler, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig,
    SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};
use tauri::async_runtime::spawn_blocking;
use tauri::ipc::Channel;
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

/// Silero VAD model inside the app resources, downloaded by `scripts/fetch-models.sh`.
const VAD_MODEL: &str = "models/silero_vad.onnx";
/// Silero VAD only accepts 16 kHz, so the worker resamples the microphone to this rate.
const VAD_SAMPLE_RATE: i32 = 16_000;
/// Silero VAD's window size at 16 kHz (32 ms).
const VAD_WINDOW: usize = 512;
/// Longest speech segment (15 s). Silero's `max_speech_duration` only makes the VAD split more
/// eagerly; continuous speech, or a TV in the background, can still extend a segment indefinitely,
/// so the worker cuts it here. Bounds both the wait for a transcript and its decode cost.
const MAX_SEGMENT_SAMPLES: usize = 15 * VAD_SAMPLE_RATE as usize;

#[derive(Default)]
pub struct Speech {
    /// Loaded on first use and reused by every session.
    recognizer: OnceLock<Arc<OfflineRecognizer>>,
    session: Mutex<Option<Session>>,
}

impl Speech {
    /// Returns the recognizer, loading the model on first use.
    async fn recognizer(&self, app: &AppHandle) -> Result<Arc<OfflineRecognizer>, String> {
        if let Some(recognizer) = self.recognizer.get() {
            return Ok(Arc::clone(recognizer));
        }
        let model_dir = resolve_resource(app, MODEL_DIR)?;
        let recognizer = spawn_blocking(move || load_recognizer(&model_dir))
            .await
            .map_err(|e| e.to_string())??;
        // If two first calls raced, both loaded the model; keep the one stored first.
        Ok(Arc::clone(
            self.recognizer.get_or_init(|| Arc::new(recognizer)),
        ))
    }
}

/// Opens the microphone and sends each transcribed speech segment over `on_transcript`, in order,
/// until `stop_listening`.
#[tauri::command]
pub async fn start_listening(
    app: AppHandle,
    speech: State<'_, Speech>,
    on_transcript: Channel<String>,
) -> Result<(), String> {
    let recognizer = speech.recognizer(&app).await?;
    let vad_model = resolve_resource(&app, VAD_MODEL)?;
    let vad = spawn_blocking(move || load_vad(&vad_model))
        .await
        .map_err(|e| e.to_string())??;

    let mut session = speech.session.lock().unwrap();
    if session.is_some() {
        return Err("Already listening.".into());
    }
    *session = Some(Session::start(recognizer, vad, on_transcript)?);
    Ok(())
}

/// Closes the microphone and returns once the trailing speech has been transcribed and sent.
#[tauri::command]
pub async fn stop_listening(speech: State<'_, Speech>) -> Result<(), String> {
    let session = speech
        .session
        .lock()
        .unwrap()
        .take()
        .ok_or("Not listening.")?;
    spawn_blocking(move || session.finish())
        .await
        .map_err(|e| e.to_string())?
}

/// An open microphone stream and the worker thread that transcribes it.
struct Session {
    /// Capture runs until the stream is dropped, which also ends the worker's audio input.
    stream: cpal::Stream,
    worker: JoinHandle<Result<(), String>>,
}

impl Session {
    fn start(
        recognizer: Arc<OfflineRecognizer>,
        vad: VoiceActivityDetector,
        on_transcript: Channel<String>,
    ) -> Result<Self, String> {
        let (stream, sample_rate, chunks) = open_microphone()?;
        let resampler = if sample_rate == VAD_SAMPLE_RATE {
            None
        } else {
            Some(
                LinearResampler::create(sample_rate, VAD_SAMPLE_RATE).ok_or_else(|| {
                    format!("Couldn't resample the microphone's {sample_rate} Hz audio.")
                })?,
            )
        };
        let worker = thread::Builder::new()
            .name("speech-session".into())
            .spawn(move || {
                transcribe_segments(
                    chunks,
                    resampler,
                    &vad,
                    &recognizer,
                    &on_transcript,
                    MAX_SEGMENT_SAMPLES,
                )
            })
            .map_err(|e| format!("Couldn't start speech recognition: {e}"))?;
        Ok(Self { stream, worker })
    }

    /// Closes the microphone and waits for the worker to finish the trailing speech.
    fn finish(self) -> Result<(), String> {
        drop(self.stream);
        self.worker
            .join()
            .map_err(|_| "Speech recognition stopped unexpectedly.")?
    }
}

/// Starts capturing the default input device at its native config, downmixed to mono. Returns the
/// stream, its sample rate, and the captured chunks; the channel closes when the stream drops.
fn open_microphone() -> Result<(cpal::Stream, i32, Receiver<Vec<f32>>), String> {
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
                // Fails only if the worker stopped early; `stop_listening` reports why.
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

/// Feeds the audio to the VAD and sends the transcript of each finished speech segment, cutting
/// segments at `max_segment_samples`. Runs until the audio channel closes, then flushes the VAD so
/// trailing speech is transcribed too.
fn transcribe_segments(
    chunks: Receiver<Vec<f32>>,
    resampler: Option<LinearResampler>,
    vad: &VoiceActivityDetector,
    recognizer: &OfflineRecognizer,
    on_transcript: &Channel<String>,
    max_segment_samples: usize,
) -> Result<(), String> {
    // 16 kHz samples that don't fill a VAD window yet.
    let mut pending = Vec::new();
    // Samples fed while the VAD has been extending its current speech segment.
    let mut segment_samples = 0;
    let mut heard_audio = false;

    for chunk in chunks {
        heard_audio |= chunk.iter().any(|&sample| sample != 0.0);
        match &resampler {
            Some(resampler) => pending.extend(resampler.resample(&chunk, false)),
            None => pending.extend(chunk),
        }
        let full_windows = pending.len() - pending.len() % VAD_WINDOW;
        for window in pending[..full_windows].chunks_exact(VAD_WINDOW) {
            vad.accept_waveform(window);
            segment_samples = if vad.detected() {
                segment_samples + VAD_WINDOW
            } else {
                0
            };
            if segment_samples >= max_segment_samples {
                // Flushing mid-speech leaves the VAD inconsistent: a pause shortly after yields a
                // negative-length segment. Send the cut segment, then reset so the next starts
                // cleanly (reset also drops queued segments, hence sending first).
                vad.flush();
                send_finished_segments(vad, recognizer, on_transcript)?;
                vad.reset();
                segment_samples = 0;
            }
        }
        pending.drain(..full_windows);
        send_finished_segments(vad, recognizer, on_transcript)?;
    }

    vad.flush();
    send_finished_segments(vad, recognizer, on_transcript)?;

    // CoreAudio doesn't report a denied microphone; it delivers silence instead.
    if !heard_audio {
        return Err(
            "No audio was captured. Allow microphone access in System Settings → \
                    Privacy & Security → Microphone, then try again."
                .into(),
        );
    }
    Ok(())
}

/// Transcribes and sends every speech segment the VAD has finished, oldest first.
fn send_finished_segments(
    vad: &VoiceActivityDetector,
    recognizer: &OfflineRecognizer,
    on_transcript: &Channel<String>,
) -> Result<(), String> {
    while let Some(segment) = vad.front() {
        let text = transcribe(recognizer, VAD_SAMPLE_RATE, segment.samples())?;
        vad.pop();
        if !text.is_empty() {
            on_transcript
                .send(text)
                .map_err(|e| format!("Couldn't send a transcript: {e}"))?;
        }
    }
    Ok(())
}

fn resolve_resource(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    app.path()
        .resolve(path, BaseDirectory::Resource)
        .map_err(|e| format!("Couldn't locate {path}: {e}"))
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

/// Loads a Silero VAD for one session; it keeps state between windows, so it isn't shared.
fn load_vad(model: &Path) -> Result<VoiceActivityDetector, String> {
    if !model.is_file() {
        return Err(format!("VAD model file is missing: {}", model.display()));
    }
    let config = VadModelConfig {
        silero_vad: SileroVadModelConfig {
            model: Some(model.to_string_lossy().into_owned()),
            threshold: 0.5,
            // The pause that ends a segment. Upstream examples use 0.1 s, which splits sentences
            // too eagerly; raise this if sentences get cut into clauses.
            min_silence_duration: 0.3,
            min_speech_duration: 0.25,
            window_size: VAD_WINDOW as i32,
            // Past this, the VAD ends the segment at the first 0.1 s dip in speech. Not a hard
            // limit; see MAX_SEGMENT_SAMPLES.
            max_speech_duration: 8.0,
        },
        sample_rate: VAD_SAMPLE_RATE,
        num_threads: 1,
        ..Default::default()
    };
    // Holds the audio of the segment in progress, which MAX_SEGMENT_SAMPLES keeps well below this.
    VoiceActivityDetector::create(&config, 30.0)
        .ok_or_else(|| format!("Couldn't load the VAD model from {}", model.display()))
}

/// Decodes one complete recording or speech segment.
fn transcribe(
    recognizer: &OfflineRecognizer,
    sample_rate: i32,
    samples: &[f32],
) -> Result<String, String> {
    let started = Instant::now();
    // A fresh stream per segment: sherpa-onnx exits the process if one stream receives two
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
    use tauri::ipc::InvokeResponseBody;

    fn model_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(MODEL_DIR)
    }

    fn read_test_wav(name: &str) -> sherpa_onnx::Wave {
        let path = model_dir().join("test_wavs").join(name);
        sherpa_onnx::Wave::read(path.to_str().unwrap()).unwrap()
    }

    #[test]
    fn transcribes_mixed_language_speech_with_punctuation() {
        let recognizer = load_recognizer(&model_dir()).unwrap();
        let wave = read_test_wav("0.wav");

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

    /// Runs the session worker over `audio`, delivered in 10 ms chunks as CoreAudio does, and
    /// returns the transcripts it sent.
    fn run_worker(
        audio: &[f32],
        sample_rate: i32,
        max_segment_samples: usize,
    ) -> Result<Vec<String>, String> {
        let recognizer = load_recognizer(&model_dir()).unwrap();
        let vad = load_vad(&Path::new(env!("CARGO_MANIFEST_DIR")).join(VAD_MODEL)).unwrap();
        let resampler = (sample_rate != VAD_SAMPLE_RATE)
            .then(|| LinearResampler::create(sample_rate, VAD_SAMPLE_RATE).unwrap());
        let (sender, chunks) = mpsc::channel();
        for chunk in audio.chunks(sample_rate as usize / 100) {
            sender.send(chunk.to_vec()).unwrap();
        }
        drop(sender);
        let transcripts = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&transcripts);
        let on_transcript = Channel::new(move |body| {
            let InvokeResponseBody::Json(json) = body else {
                panic!("expected a JSON message");
            };
            received
                .lock()
                .unwrap()
                .push(serde_json::from_str::<String>(&json).unwrap());
            Ok(())
        });

        transcribe_segments(
            chunks,
            resampler,
            &vad,
            &recognizer,
            &on_transcript,
            max_segment_samples,
        )?;
        Ok(transcripts.lock().unwrap().clone())
    }

    /// Streams two utterances, each after a pause, at 48 kHz like a typical microphone, and closes
    /// the audio right after the second one, without trailing silence.
    #[test]
    fn transcribes_each_segment_in_order_including_trailing_speech() {
        let first = read_test_wav("2.wav");
        let second = read_test_wav("1.wav");
        let pause = vec![0.0; first.sample_rate() as usize];
        let audio_16k = [&pause, first.samples(), &pause, second.samples()].concat();
        let audio_48k = LinearResampler::create(16_000, 48_000)
            .unwrap()
            .resample(&audio_16k, true);

        let transcripts = run_worker(&audio_48k, 48_000, MAX_SEGMENT_SAMPLES).unwrap();

        // Punctuation can differ slightly from decoding each file whole, so check the endings.
        assert_eq!(transcripts.len(), 2, "{transcripts:?}");
        assert!(
            transcripts[0].ends_with("frequently 频繁的。"),
            "{transcripts:?}"
        );
        assert!(transcripts[1].ends_with("什么意思啊？"), "{transcripts:?}");
    }

    /// Cutting a segment mid-speech once left the VAD producing an empty segment, whose decode
    /// aborted the process.
    #[test]
    fn cuts_speech_without_pauses_at_the_segment_cap() {
        let audio: Vec<f32> = ["0.wav", "3.wav", "2.wav", "1.wav"]
            .into_iter()
            .flat_map(|name| read_test_wav(name).samples().to_vec())
            .collect();
        let two_seconds = 2 * VAD_SAMPLE_RATE as usize;

        let uncut = run_worker(&audio, VAD_SAMPLE_RATE, MAX_SEGMENT_SAMPLES).unwrap();
        let cut = run_worker(&audio, VAD_SAMPLE_RATE, two_seconds).unwrap();

        assert!(cut.len() > uncut.len(), "{uncut:?}\n{cut:?}");
        assert!(cut.last().unwrap().ends_with("什么意思啊？"), "{cut:?}");
    }

    #[test]
    fn reports_silence_as_a_microphone_access_error() {
        let silence = vec![0.0; VAD_SAMPLE_RATE as usize];

        let error = run_worker(&silence, VAD_SAMPLE_RATE, MAX_SEGMENT_SAMPLES).unwrap_err();

        assert!(error.starts_with("No audio was captured."));
    }
}
