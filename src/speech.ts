// Typed calls to the speech commands in src-tauri/src/commands.rs. `invoke` doesn't check command names,
// argument casing or payloads at runtime, so keep this file aligned with the Rust signatures.
import { Channel, invoke } from "@tauri-apps/api/core";

/** Creates the reusable speech session if needed, without opening the microphone. */
export function createSession(): Promise<void> {
  return invoke("create_session");
}

/**
 * Opens the microphone in the created session and calls `onTranscript` with each segment, in order,
 * until `stopListening`. Rejects with an error message if listening couldn't start.
 */
export function startListening(onTranscript: (transcript: string) => void): Promise<void> {
  return invoke("start_listening", { onTranscript: new Channel<string>(onTranscript) });
}

/**
 * Closes the microphone and resolves once the trailing speech has been transcribed and sent.
 * Rejects with an error message if listening failed or wasn't running.
 */
export function stopListening(): Promise<void> {
  return invoke("stop_listening");
}
