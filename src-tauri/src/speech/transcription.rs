//! Splits microphone audio into bounded speech segments and transcribes them in order.

use std::path::Path;
use std::sync::mpsc::Receiver;

use sherpa_onnx::{
    LinearResampler, OfflineRecognizer, SileroVadModelConfig, VadModelConfig, VoiceActivityDetector,
};

use super::transcribe;

/// Silero VAD only accepts 16 kHz, so the worker resamples the microphone to this rate.
pub(super) const SAMPLE_RATE: i32 = 16_000;
/// Silero VAD's window size at 16 kHz (32 ms).
const VAD_WINDOW: usize = 512;
/// Longest speech segment (15 s). Silero's `max_speech_duration` only makes the VAD split more
/// eagerly; continuous speech, or a TV in the background, can still extend a segment indefinitely,
/// so the worker cuts it here. Bounds both the wait for a transcript and its decode cost.
pub(super) const MAX_SEGMENT_SAMPLES: usize = 15 * SAMPLE_RATE as usize;

/// Feeds the audio to the VAD and sends the transcript of each finished speech segment, cutting
/// segments at `max_segment_samples`. Runs until the audio channel closes, then flushes the VAD so
/// trailing speech is transcribed too.
pub(super) fn run(
    chunks: Receiver<Vec<f32>>,
    resampler: Option<LinearResampler>,
    vad: &VoiceActivityDetector,
    recognizer: &OfflineRecognizer,
    mut on_transcript: impl FnMut(String) -> Result<(), String>,
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
                send_finished_segments(vad, recognizer, &mut on_transcript)?;
                vad.reset();
                segment_samples = 0;
            }
        }
        pending.drain(..full_windows);
        send_finished_segments(vad, recognizer, &mut on_transcript)?;
    }

    vad.flush();
    send_finished_segments(vad, recognizer, &mut on_transcript)?;

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
    on_transcript: &mut impl FnMut(String) -> Result<(), String>,
) -> Result<(), String> {
    while let Some(segment) = vad.front() {
        let text = transcribe(recognizer, SAMPLE_RATE, segment.samples())?;
        vad.pop();
        if !text.is_empty() {
            on_transcript(text)?;
        }
    }
    Ok(())
}

/// Loads a Silero VAD for one session; it keeps state between windows, so it isn't shared.
pub(super) fn load_vad(model: &Path) -> Result<VoiceActivityDetector, String> {
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
        sample_rate: SAMPLE_RATE,
        num_threads: 1,
        ..Default::default()
    };
    // Holds the audio of the segment in progress, which MAX_SEGMENT_SAMPLES keeps well below this.
    VoiceActivityDetector::create(&config, 30.0)
        .ok_or_else(|| format!("Couldn't load the VAD model from {}", model.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::speech::{MODEL_DIR, load_recognizer};
    use std::path::PathBuf;
    use std::sync::mpsc;

    /// The models that `scripts/fetch-models.sh` downloads, which the app bundles as resources.
    fn models_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("models")
    }

    fn model_dir() -> PathBuf {
        models_dir().join(MODEL_DIR)
    }

    fn read_test_wav(name: &str) -> sherpa_onnx::Wave {
        let path = model_dir().join("test_wavs").join(name);
        sherpa_onnx::Wave::read(path.to_str().unwrap()).unwrap()
    }

    /// Runs the session worker over `audio`, delivered in 10 ms chunks as CoreAudio does, and
    /// returns the transcripts it sent.
    fn run_worker(
        audio: &[f32],
        sample_rate: i32,
        max_segment_samples: usize,
    ) -> Result<Vec<String>, String> {
        let recognizer = load_recognizer(&model_dir()).unwrap();
        let vad = load_vad(&models_dir().join("silero_vad.onnx")).unwrap();
        let resampler = (sample_rate != SAMPLE_RATE)
            .then(|| LinearResampler::create(sample_rate, SAMPLE_RATE).unwrap());
        let (sender, chunks) = mpsc::channel();
        for chunk in audio.chunks(sample_rate as usize / 100) {
            sender.send(chunk.to_vec()).unwrap();
        }
        drop(sender);
        let mut transcripts = Vec::new();

        run(
            chunks,
            resampler,
            &vad,
            &recognizer,
            |text| {
                transcripts.push(text);
                Ok(())
            },
            max_segment_samples,
        )?;
        Ok(transcripts)
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
        let two_seconds = 2 * SAMPLE_RATE as usize;

        let uncut = run_worker(&audio, SAMPLE_RATE, MAX_SEGMENT_SAMPLES).unwrap();
        let cut = run_worker(&audio, SAMPLE_RATE, two_seconds).unwrap();

        assert!(cut.len() > uncut.len(), "{uncut:?}\n{cut:?}");
        assert!(cut.last().unwrap().ends_with("什么意思啊？"), "{cut:?}");
    }

    #[test]
    fn reports_silence_as_a_microphone_access_error() {
        let silence = vec![0.0; SAMPLE_RATE as usize];

        let error = run_worker(&silence, SAMPLE_RATE, MAX_SEGMENT_SAMPLES).unwrap_err();

        assert!(error.starts_with("No audio was captured."));
    }
}
