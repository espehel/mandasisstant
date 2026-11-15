//! On-device speech recognition. A session owns the recognizer and starts or stops listening.

mod microphone;
mod session;
mod transcription;

use std::path::Path;
use std::time::Instant;

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

pub use session::Session;

/// X-ASR model inside the models directory, downloaded by `scripts/fetch-models.sh`.
const MODEL_DIR: &str = "sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03";
const ENCODER: &str = "encoder-epoch-99-avg-1.int8.onnx";
/// Upstream ships an fp32 decoder in the int8 package on purpose.
const DECODER: &str = "decoder-epoch-99-avg-1.onnx";
const JOINER: &str = "joiner-epoch-99-avg-1.int8.onnx";
const TOKENS: &str = "tokens.txt";
/// Tune with the decode time that `transcribe` logs.
const NUM_THREADS: i32 = 2;

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

    #[test]
    fn names_the_missing_model_file() {
        let error = load_recognizer(Path::new("/nonexistent")).err().unwrap();

        assert_eq!(
            error,
            format!("Speech model file is missing: /nonexistent/{ENCODER}")
        );
    }

    #[test]
    fn transcribes_mixed_language_speech_with_punctuation() {
        let model_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("models")
            .join(MODEL_DIR);
        let recognizer = load_recognizer(&model_dir).unwrap();
        let wave_path = model_dir.join("test_wavs/0.wav");
        let wave = sherpa_onnx::Wave::read(wave_path.to_str().unwrap()).unwrap();

        let text = transcribe(&recognizer, wave.sample_rate(), wave.samples()).unwrap();

        assert_eq!(
            text,
            "昨天是 Monday, today is 礼拜二， the day after tomorrow 是星期三。"
        );
    }
}
