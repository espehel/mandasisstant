# Mandasisstant

macOS desktop app that translates between English and Mandarin Chinese from speech or text,
showing Chinese characters and pinyin. Speech recognition runs on-device with
[sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx), and translation uses Apple's on-device
Translation framework: offline, no API costs.

## Requirements

- macOS 26 (Tahoe) or later, Apple Silicon
- Swift 6.2+ toolchain (Xcode or the Command Line Tools)
- Rust (stable) and Node.js + npm

## Layout

| Path | What |
| --- | --- |
| `src/` | React + TypeScript frontend (Vite) |
| `src-tauri/` | Tauri v2 Rust backend; `build.rs` compiles and links the Swift package via swift-rs |
| `src-tauri/src/speech.rs` | Listening sessions: microphone capture (cpal), voice activity detection and speech recognition (sherpa-onnx) |
| `scripts/fetch-models.sh` | Downloads the speech and VAD models into `src-tauri/models/` (gitignored) |
| `AppleBridge/` | Swift package |
| `AppleBridge/Sources/AppleBridgeCore` | Translation and microphone-permission logic as a plain async Swift API |
| `AppleBridge/Sources/AppleBridgeFFI` | Thin `@_cdecl` wrappers around Core, called from Rust |
| `AppleBridge/Sources/bridge-test` | CLI that calls Core directly, for testing without Tauri |

### Deployment target

The minimum macOS version (26.0) is set in four places, and `src-tauri/build.rs` fails the
build if they disagree:

- `AppleBridge/Package.swift`: `platforms: [.macOS(.v26)]`
- `src-tauri/build.rs`: `MACOS_MIN_VERSION` (passed to swift-rs `SwiftLinker`)
- `src-tauri/tauri.conf.json`: `bundle.macOS.minimumSystemVersion`
- `src-tauri/.cargo/config.toml`: `MACOSX_DEPLOYMENT_TARGET` (`tauri dev` ignores the bundle setting)

## Build and run

```sh
npm install
./scripts/fetch-models.sh   # once: speech and VAD models (~137 MB download), checksum-verified
npm run tauri dev           # dev mode with hot reload
npm run tauri build         # release .app in src-tauri/target/release/bundle/macos/
```

The app bundles `src-tauri/models/` as a resource, so the build fails until the fetch script
has run. The first Rust build also needs network access: the `sherpa-onnx` crate downloads its
prebuilt static library (which includes onnxruntime) into `target/`. After that, everything
runs offline.

The Swift package is built automatically by `src-tauri/build.rs` (into Cargo's `OUT_DIR`) and
rebuilt whenever anything under `AppleBridge/` changes. That includes `AppleBridge/.build`, so
running `swift build` there also triggers a short Rust rebuild on the next `cargo`/`tauri` run.

## bridge-test

Exercise the Swift core directly, without Tauri:

```sh
cd AppleBridge
swift build
swift run bridge-test
```

When run from Terminal, macOS attributes microphone permission prompts to Terminal (or your
terminal app), not to Mandasisstant.

## Translation language packs

Translation runs fully on-device and needs the English and Chinese (Simplified) language
packs installed:

**System Settings → General → Language & Region → Translation Languages**, then download
**English** and **Chinese (Simplified)**.

If they're missing, the app reports a clear error (added in Phase 5).

## Microphone permission

macOS asks for microphone access the first time you start a session. Under `npm run tauri dev`,
the prompt and the grant belong to your terminal app; in the bundled app, they belong to
Mandasisstant. If access is denied, the microphone delivers only silence, and the app shows a
microphone-access error when you pause or leave the session. Change it in **System Settings →
Privacy & Security → Microphone**, or reset the bundled app's permission with:

```sh
tccutil reset Microphone no.noteless.mandasisstant
```

## Credits

- [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx) (Apache-2.0): on-device speech recognition
- [X-ASR](https://github.com/Gilgamesh-J/X-ASR) (Apache-2.0): bilingual Chinese–English speech model
- [Silero VAD](https://github.com/snakers4/silero-vad) (MIT): voice activity detection
