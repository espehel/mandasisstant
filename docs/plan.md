# Project: macOS English ↔ Mandarin translator (Tauri + Apple Translation + sherpa-onnx)

## Goal
Build a macOS desktop app that translates between English and Mandarin Chinese from speech or text, showing Chinese characters and pinyin. Use Apple's on-device Translation framework for translation and sherpa-onnx (on-device ONNX speech recognition) for speech-to-text, so the app runs offline at zero API cost.

## Architecture
- **Tauri v2** app with a React + TypeScript frontend.
- **Speech recognition in Rust** (`src-tauri/src/speech.rs`): the official `sherpa-onnx` crate handles voice activity detection (VAD) and recognition, and `cpal` captures the microphone.
- **Swift package** (`AppleBridge/`) in the same repo, linked into the Rust backend with the **swift-rs** crate (compiled via `SwiftLinker` in `build.rs`). It covers Apple framework work only: translation and the microphone permission request.
- Swift package targets:
  - `AppleBridgeCore`: translation and microphone-permission logic as a clean, plain async Swift API. No FFI concerns here.
  - `AppleBridgeFFI`: thin `@_cdecl` wrappers that only convert types and forward calls to Core.
  - `bridge-test`: small executable that calls Core directly and prints results, for testing without Tauri.
- Rust exposes Tauri commands: `start_listening`, `stop_listening`, `translate`. From Phase 3 on, transcripts stream to the frontend over a `tauri::ipc::Channel` passed to `start_listening` (ordered, scoped to one session, no global listener to clean up).
- Pinyin is generated in the frontend with the `pinyin-pro` npm package.

Speech data flow (from Phase 3):

```text
cpal input callback (device's native rate, downmixed to mono f32)
  → std mpsc channel
  → session worker thread (owns the VAD and uses the shared recognizer)
      resample to 16 kHz → Silero VAD (512-sample windows)
      each finished speech segment → X-ASR OfflineRecognizer → transcript
  → Channel<string> → React
      (Phase 7: → invoke("translate") → Swift Translation → pinyin-pro)
```

## Speech recognition design
Researched 2026-11-08. Decisions and reasons:

- **Binding: the official `sherpa-onnx` crate (2.0.x, published by the sherpa-onnx maintainer) with its default `static` feature.** Its build script downloads a prebuilt macOS arm64 static library (~21 MB, includes onnxruntime 1.28) on the first build and caches it in `target/`. That means no cmake, no dylibs to bundle, and no rpath or extra code-signing work. The community `sherpa-rs` crate is archived and points to the official one.
- **Model: X-ASR-zh-en, offline Zipformer transducer with punctuation, int8** (`sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03.tar.bz2`, ~130 MB download, ~175 MB on disk, Apache-2.0).
  - One bilingual model covers both directions and handles mixed speech.
  - It outputs punctuation and English casing, which the translation step needs, and simplified Chinese.
  - Authors' benchmarks: LibriSpeech clean/other WER 2.56 / 5.56, WenetSpeech net/meeting CER 5.83 / 7.06, which on average beats SenseVoice-small.
  - It loads as an `OfflineRecognizer` with a `transducer` (encoder/decoder/joiner) config.
- **Two steps:**
  - **Push-to-talk (Phase 2).** Buffer the whole recording and decode it once on stop. This is the simplest end-to-end path through mic, model, and UI.
  - **Sessions with VAD (Phase 3).** The mic stays open for the whole session. VAD splits speech into segments at pauses, and each segment is decoded as soon as it ends. The worker caps segment length at 15 s, so decode cost stays small however long the session runs.
- **VAD: Silero** (`silero_vad.onnx` from the same `asr-models` release; k2-fsa's v4 export, ~630 KB, MIT).
  - It only accepts 16 kHz. Unlike the recognizer it doesn't resample, so the worker resamples first with `sherpa_onnx::LinearResampler`.
  - Settings: threshold 0.5, min speech 0.25 s, min silence 0.3 s, max speech 8 s, window 512 samples.
  - Max speech is not a hard limit: past it, sherpa-onnx only raises the threshold to 0.9 and ends the segment at the first 0.1 s dip. Continuous speech (or a TV in the background) can extend a segment indefinitely, so the worker flushes the VAD at 15 s, sends that segment, and resets the VAD. Flushing without the reset can yield a negative-length segment whose decode aborts the process.
  - The official examples use 0.1 s min silence, which splits sentences too eagerly.
- **Audio: `cpal` 0.18.** Open the default input device at its native config (usually 48 kHz f32) and downmix to mono in the callback. The recognizer resamples internally, so no resampling crate is needed.
- **CPU execution provider.** The static macOS build has no CoreML support, and upstream reports show CoreML gives no speedup on M-series chips.

Alternatives considered:

| Option | Why not (for now) |
| --- | --- |
| X-ASR 480 ms streaming export (`…-480ms-streaming-zipformer-transducer-zh-en-punct-int8-2026-06-05`) | Only worth it for live partial transcripts (see Nice to have), and only if re-decoding is too slow. It has constant decode cost but worse finals (WenetSpeech net CER 7.46 vs 5.83). |
| SenseVoice-small int8 (`…-2024-07-17`, with `use_itn`) | **Fallback** if X-ASR's output disappoints. Mature, and the same `OfflineRecognizer` code works with a different config. English is weaker and it uses the FunASR model license. |
| Qwen3-ASR 0.6B int8 | Most accurate, but ~840 MB. |
| 2023 bilingual streaming Zipformer / Paraformer | No punctuation and lower accuracy. |
| sherpa-onnx Swift API inside `AppleBridgeCore` | Needs an xcframework in SwiftPM plus Swift → Rust streaming callbacks. The Rust crate avoids both. |
| `shared` linking | Means shipping two dylibs in `Contents/Frameworks` and handling rpath. Static avoids both. |

Unverified until Phases 2–3:
- Decode speed on the target Mac, which sets how long a transcript takes to appear after each pause. No Apple Silicon numbers are published.
- X-ASR's actual punctuation and simplified-character output.
- Whether audio starts flowing mid-recording once the user accepts macOS's automatic permission prompt.
- VAD settings that suit natural pauses.

## Constraints
- Target **macOS 26 (Tahoe)** minimum, on Apple Silicon. Use the same deployment target in `Package.swift`, the swift-rs `SwiftLinker` config, and `bundle.macOS.minimumSystemVersion` in `tauri.conf.json`.
- Translation framework: on macOS 26, create `TranslationSession` directly in code (no SwiftUI `.translationTask`). Use `LanguageAvailability` to check whether the English and Chinese language packs are installed, and surface a clear error if they aren't.
- Speech: on-device only, using `sherpa-onnx` (default `static` feature) and `cpal`, as described above. The first Rust build needs network access to fetch the prebuilt sherpa-onnx library.
- Model files are not committed:
  - `scripts/fetch-models.sh` downloads the pinned files, checks their pinned SHA-256 (from GitHub's release-asset digest), and extracts them into `src-tauri/models/` (gitignored).
  - The app bundles `src-tauri/models/` via `bundle.resources` and resolves it with `BaseDirectory::Resource`, which also works under `tauri dev`.
  - A missing resource fails the build, so fresh clones run the fetch script first.
  - This setup-time download is the only network use. The app itself runs offline.
- Never feed a different sample rate into an existing sherpa-onnx stream: the library exits the whole process. Fix the rate per recording or session.
- Microphone permission:
  - CoreAudio doesn't report a denied permission; a denied mic just delivers zeroed samples.
  - Until Phase 7, rely on macOS's automatic prompt on first capture, and report a recording or session that captured only zeros as a microphone-access error.
  - From Phase 7, request access explicitly through `AVAudioApplication` in `AppleBridgeCore` before opening the mic.
  - Under `tauri dev`, the prompt and the grant belong to the terminal app.
  - Keep `NSMicrophoneUsageDescription` in `src-tauri/Info.plist` and the `com.apple.security.device.audio-input` entitlement for hardened runtime.
  - Remove `NSSpeechRecognitionUsageDescription`, since the Speech framework is no longer used.
- Threading:
  - Phase 2's single decode is finite work, so run it in `spawn_blocking`.
  - Phase 3's session worker is long-lived, so it gets its own `std::thread`.
  - Only one thread at a time uses the recognizer and VAD. The cpal callback only downmixes and sends.
  - Commands that load models or call into Swift are `async`, so they stay off the main thread. Blocking FFI calls go through `spawn_blocking`.
  - Keep lock scopes short. Never hold a lock while loading a model or joining the worker.
- Strings passed across the Swift FFI must be freed correctly.
- Don't rely on memory for API details that may have changed. That includes Tauri v2 config, swift-rs usage, the sherpa-onnx Rust API, cpal 0.18, and the macOS 26 Translation and AVFAudio APIs. The official sherpa-onnx examples pin cpal 0.16: in 0.18, `play()` is required and `SampleRate` is a plain `u32`. Check current docs or crate sources when unsure.
- No paid or cloud APIs.

## Phases
Work through these in order. **After each phase, stop, summarize what you did and how to verify it, and wait for my confirmation before continuing.**

### Phase 1: Scaffold - Done
- Create the Tauri v2 app (React + TS) and the Swift package with the three targets.
- Set aligned deployment targets everywhere.
- Done when: the empty Tauri app runs with `npm run tauri dev`, and `swift build` succeeds for the package.

### Phase 2: Push-to-talk speech-to-text - Done
- Add `sherpa-onnx` to `src-tauri/Cargo.toml` and confirm it links into the app next to the Swift static library.
- Add `scripts/fetch-models.sh` (the X-ASR model for now), gitignore `src-tauri/models/`, and bundle it as a resource.
- In `speech.rs`:
  - Load the `OfflineRecognizer` once, on first use, and reuse it. `create` returns `None` on a bad config, so report a clear error naming the model directory and any missing file. Start with `num_threads = 2`.
  - Record from the default input device with cpal at its native config. In the callback, downmix to mono and send the chunk over an mpsc channel. Call `play()` on the stream.
  - On stop:
    - Drop the stream and collect the samples.
    - Decode once in `spawn_blocking` and return the text.
    - Log the decode time so `num_threads` can be tuned.
- Commands, with the active recording kept in `Mutex<Option<…>>` managed state:
  - `start_listening` loads the model if needed, then opens the mic. It rejects the call if a recording is already active.
  - `stop_listening` returns the transcript.
- Replace the template UI with a minimal one:
  - One button that toggles Record / Stop.
  - States for loading the model, recording, transcribing, the result, and errors.
  - The transcript shown when decoding finishes.
- Remove the Apple Speech leftovers: `NSSpeechRecognitionUsageDescription`, plus the "Apple Speech" wording in `README.md` and the `Cargo.toml` description.
- Done when, under `tauri dev` and in a `tauri build` bundle:
  - Pressing Record, speaking a sentence in English or Mandarin, and pressing Stop shows the correct transcript with punctuation, with Mandarin in simplified characters.
  - A denied mic shows the microphone-access error.

### Phase 3: Long-lived sessions with VAD - TODO
- Add `silero_vad.onnx` to the fetch script.
- Turn the recording into a session: the mic stays open from Start until Stop.
- A session worker thread owns the VAD and uses the shared recognizer. It:
  - resamples to 16 kHz with `LinearResampler`;
  - feeds the VAD in 512-sample windows;
  - decodes each finished segment and sends its text over a `Channel<string>` passed to `start_listening`.
- `stop_listening` drops the cpal stream, which closes the audio channel. The worker then flushes the VAD, decodes any trailing segment, and exits, and the command joins it. Phase 2's single-shot path goes away.
- UI:
  - Every screen shares a header merged into the window's title bar (`titleBarStyle: "Overlay"`, draggable): the macOS window controls, a home button back to the welcome screen, and the open session's date.
  - A welcome screen dominated by a "Start session" button, with a side menu listing past sessions (saved in the webview's `localStorage`).
  - Each session opens in its own screen: the session's transcripts as an ordered list that grows as segments arrive, and a bottom bar with a play/pause button. Pause closes the microphone; play resumes listening into the same session.
- Done when:
  - In one session, several sentences separated by pauses appear as separate transcripts, in order, in both languages.
  - Stopping mid-sentence still transcribes that sentence.
  - A session of several minutes stays responsive.

### Phase 4: Prove the FFI pipeline - TODO
- Add one trivial FFI function in Swift that returns `"hello from Swift"`.
- Call it from a Tauri command via swift-rs and display it in the frontend.
- Done when: the string shows in the UI under both `tauri dev` and a `tauri build` bundle.

### Phase 5: Core translation logic - TODO
- Implement translation in `AppleBridgeCore` (en ↔ zh-Hans), including the language-availability check.
- Make `bridge-test` translate sample strings in both directions and print the results.
- Done when: `swift run bridge-test` prints correct translations.

### Phase 6: Text translation in the app - TODO
- Add a `translate` Tauri command that calls Core through FFI, replacing the Phase 4 hello function.
- Extend the UI with:
  - A text input.
  - A language toggle. It sets the translation direction only; the speech model is bilingual and needs no language.
  - An output showing the translation, Chinese characters and pinyin (via `pinyin-pro`, with tone marks).
- Done when: typed text translates in both directions in the app, with pinyin for the Chinese side.

### Phase 7: Speech → translation in the bundled app - TODO
- Translate each session transcript as it arrives, in the toggle's direction, and show the translation and pinyin next to it.
- Microphone permission:
  - Add `requestMicrophoneAccess()` to `AppleBridgeCore` (using `AVAudioApplication`) with a thin FFI wrapper.
  - Call it in `start_listening` before opening the mic.
  - When access is denied, return a clear error that points to System Settings → Privacy & Security → Microphone.
  - This replaces the all-zero check.
- Set `bundle.macOS.signingIdentity` to `"-"` so local builds are ad-hoc signed with hardened runtime and entitlements applied. Unsigned builds skip both.
- Done when, in the bundled `.app`:
  - The first Start shows the permission prompt for Mandasisstant before recording begins.
  - Each spoken sentence shows its transcript, translation and pinyin, in both directions.
  - Denying permission shows the error.
  - To retest: `tccutil reset Microphone no.noteless.mandasisstant`.

## Conventions
- Keep `AppleBridgeFFI` thin; Apple framework logic belongs in Core. Speech logic lives in `src-tauri/src/speech.rs`.
- `sherpa-onnx` is the only ML runtime. Don't add the `ort` crate or `sherpa-rs`: their onnxruntime and protobuf symbols would clash with the static library.
- Write a short README section documenting how to build, fetch the models, run `bridge-test`, install the Translation language packs (System Settings → General → Language & Region → Translation Languages), and reset the microphone permission. Credit sherpa-onnx and X-ASR (both Apache-2.0) and Silero VAD (MIT).

## Nice to have
Not part of the phases above. Pick it up only after Phase 7, and only if I ask for it.

### Live partial transcripts
- While the VAD reports active speech, re-decode the current segment's audio about every 300 ms and send the result as a partial. The decode of the finished segment becomes the final, exactly as in Phase 3.
- Drain queued audio before each decode, so a slow decode lowers the partial rate instead of building a backlog. The worker's 15 s segment cap keeps decode cost bounded.
- Change the Channel payload to `{ kind: "partial" | "final"; text: string }`.
- In the UI, show the in-progress partial below the transcript list and replace it with the final when the segment ends. Translate finals only.
- The reference pattern is the official `sense_voice_simulate_streaming_microphone.rs` example.
- If re-decoding turns out too slow on the target Mac, use the X-ASR 480 ms streaming export (`OnlineRecognizer`) instead. It has constant decode cost but worse finals.
- Done when: text appears and updates while I'm speaking, and each segment's final matches what Phase 3 produces.

## Sources
- sherpa-onnx Rust crate: https://docs.rs/sherpa-onnx/2.0.1/sherpa_onnx/ (source in `sherpa-onnx/rust/` of https://github.com/k2-fsa/sherpa-onnx)
- Rust mic examples:
  - https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/sense_voice_simulate_streaming_microphone.rs
  - https://github.com/k2-fsa/sherpa-onnx/blob/master/rust-api-examples/examples/streaming_zipformer_microphone.rs
- Tauri mic + VAD example (closest to Phase 3): https://github.com/k2-fsa/sherpa-onnx/tree/master/tauri-examples
- Internal resampling: https://github.com/k2-fsa/sherpa-onnx/blob/master/sherpa-onnx/csrc/features.cc
- Model and VAD files: https://github.com/k2-fsa/sherpa-onnx/releases/tag/asr-models
- Silero VAD in sherpa-onnx: https://k2-fsa.github.io/sherpa/onnx/vad/silero-vad.html
- X-ASR: https://github.com/Gilgamesh-J/X-ASR, https://huggingface.co/GilgameshWind/X-ASR-zh-en
- `sherpa-rs` (archived): https://github.com/thewh1teagle/sherpa-rs
- cpal changes: https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md
- Mic permission behavior: https://developer.apple.com/documentation/avfaudio/avaudioapplication/requestrecordpermission(completionhandler:)
- Tauri channels and resources: https://v2.tauri.app/develop/calling-frontend/, https://v2.tauri.app/develop/resources/
