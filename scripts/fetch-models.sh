#!/bin/sh
# Downloads the pinned speech models into src-tauri/models/ (gitignored, bundled as an app resource).
# Run once after cloning; `tauri dev`/`tauri build` fail until the models are present.
set -eu

RELEASE_URL="https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models"
MODELS_DIR="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/models"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# Usage: fetch_file <file name> <sha256 from the GitHub release asset digest>
fetch_file() {
  name="$1"
  sha256="$2"
  if [ -f "$MODELS_DIR/$name" ]; then
    echo "$name: already present"
    return
  fi

  echo "$name: downloading"
  curl -fL --retry 3 --progress-bar -o "$tmp/$name" "$RELEASE_URL/$name"
  echo "$sha256  $tmp/$name" | shasum -a 256 -c -
  mkdir -p "$MODELS_DIR"
  mv "$tmp/$name" "$MODELS_DIR/$name"
}

# Usage: fetch_archive <archive name without .tar.bz2> <sha256 from the GitHub release asset digest>
fetch_archive() {
  name="$1"
  sha256="$2"
  if [ -d "$MODELS_DIR/$name" ]; then
    echo "$name: already present"
    return
  fi

  echo "$name: downloading"
  curl -fL --retry 3 --progress-bar -o "$tmp/$name.tar.bz2" "$RELEASE_URL/$name.tar.bz2"
  echo "$sha256  $tmp/$name.tar.bz2" | shasum -a 256 -c -
  tar -xjf "$tmp/$name.tar.bz2" -C "$tmp"
  # Move into place only after a complete extraction, so an interrupted run is retried.
  mkdir -p "$MODELS_DIR"
  mv "$tmp/$name" "$MODELS_DIR/$name"
}

# X-ASR zh-en offline Zipformer transducer with punctuation, int8 (Apache-2.0).
fetch_archive sherpa-onnx-x-asr-zipformer-transducer-zh-en-punct-int8-2026-06-03 \
  5d02c36d7b44e886b7c8f0d8e051f8713acab96c264bb6ef9e718be39a6a2224

# Silero VAD, k2-fsa's v4 export (MIT).
fetch_file silero_vad.onnx \
  9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6
