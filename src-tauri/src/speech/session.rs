//! A reusable speech session. Creating it loads the recognizer; listening can start and stop
//! repeatedly without reloading it. All methods run on a blocking worker, outside the UI thread.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

use sherpa_onnx::{LinearResampler, OfflineRecognizer};

use super::{MODEL_DIR, load_recognizer, microphone, transcription};

pub struct Session {
    recognizer: Arc<OfflineRecognizer>,
    models_dir: PathBuf,
    listening: Option<Listening>,
}

struct Listening {
    stream: cpal::Stream,
    worker: JoinHandle<Result<(), String>>,
}

impl Session {
    pub fn new(models_dir: &Path) -> Result<Self, String> {
        Ok(Self {
            recognizer: Arc::new(load_recognizer(&models_dir.join(MODEL_DIR))?),
            models_dir: models_dir.to_owned(),
            listening: None,
        })
    }

    /// Opens the microphone and sends transcripts in order on a dedicated worker thread.
    /// Callback failures end the worker and are returned by `stop_listening`.
    pub fn start_listening(
        &mut self,
        on_transcript: impl FnMut(String) -> Result<(), String> + Send + 'static,
    ) -> Result<(), String> {
        if self.listening.is_some() {
            return Err("Already listening.".into());
        }

        let vad = transcription::load_vad(&self.models_dir.join("silero_vad.onnx"))?;
        let (stream, sample_rate, chunks) = microphone::open()?;
        let resampler = if sample_rate == transcription::SAMPLE_RATE {
            None
        } else {
            Some(
                LinearResampler::create(sample_rate, transcription::SAMPLE_RATE).ok_or_else(
                    || format!("Couldn't resample the microphone's {sample_rate} Hz audio."),
                )?,
            )
        };
        let recognizer = Arc::clone(&self.recognizer);
        let worker = thread::Builder::new()
            .name("speech-listening".into())
            .spawn(move || {
                transcription::run(
                    chunks,
                    resampler,
                    &vad,
                    &recognizer,
                    on_transcript,
                    transcription::MAX_SEGMENT_SAMPLES,
                )
            })
            .map_err(|e| format!("Couldn't start speech recognition: {e}"))?;
        self.listening = Some(Listening { stream, worker });
        Ok(())
    }

    /// Closes the microphone and waits for trailing transcripts. The session remains reusable,
    /// including when the worker returns an error.
    pub fn stop_listening(&mut self) -> Result<(), String> {
        let Listening { stream, worker } = self.listening.take().ok_or("Not listening.")?;
        // Dropping the stream closes the worker's audio channel so it can drain and exit.
        drop(stream);
        worker
            .join()
            .map_err(|_| "Speech recognition stopped unexpectedly.")?
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self.listening.is_some()
            && let Err(error) = self.stop_listening()
        {
            eprintln!("Couldn't finish the speech session: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_a_session_without_starting_the_microphone() {
        let models_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
        let mut session = Session::new(&models_dir).unwrap();

        assert!(session.listening.is_none());
        assert_eq!(session.stop_listening().unwrap_err(), "Not listening.");
    }

    #[test]
    fn failed_start_leaves_the_session_ready_to_retry() {
        let models_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("models");
        let mut session = Session::new(&models_dir).unwrap();
        // Fail before opening a real microphone, without invalidating the loaded recognizer.
        session.models_dir = PathBuf::from("/nonexistent");

        for _ in 0..2 {
            assert_eq!(
                session.start_listening(|_| Ok(())).unwrap_err(),
                "VAD model file is missing: /nonexistent/silero_vad.onnx"
            );
            assert!(session.listening.is_none());
        }
    }
}
