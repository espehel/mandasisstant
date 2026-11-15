//! Tauri command handlers. They only adapt IPC to the Tauri-independent logic behind them:
//! resolve bundled paths, move blocking work off the async runtime, and turn channels into
//! callbacks. Each command's TypeScript call lives in the matching module in `src/`.

use std::sync::Arc;

use tauri::async_runtime::{Mutex, spawn_blocking};
use tauri::ipc::Channel;
use tauri::path::BaseDirectory;
use tauri::{AppHandle, Manager, State};

use crate::speech::Session;

// Async callers wait without blocking a runtime thread. The owned guard moves into
// spawn_blocking, keeping create/start/stop serialized until all native work has finished.
pub type SessionState = Arc<Mutex<Option<Session>>>;

/// Bundled from `src-tauri/models/` by `bundle.resources` in tauri.conf.json.
const MODELS_DIR: &str = "models";

/// Creates the reusable speech session without opening the microphone. Repeated calls reuse it.
#[tauri::command]
pub async fn create_session(app: AppHandle, state: State<'_, SessionState>) -> Result<(), String> {
    let mut session = Arc::clone(&state).lock_owned().await;
    if session.is_some() {
        return Ok(());
    }
    let models_dir = app
        .path()
        .resolve(MODELS_DIR, BaseDirectory::Resource)
        .map_err(|e| format!("Couldn't locate the speech models: {e}"))?;
    spawn_blocking(move || {
        *session = Some(Session::new(&models_dir)?);
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Opens the microphone and sends each transcribed speech segment over `on_transcript`, in order,
/// until `stop_listening`.
#[tauri::command]
pub async fn start_listening(
    state: State<'_, SessionState>,
    on_transcript: Channel<String>,
) -> Result<(), String> {
    let mut session = Arc::clone(&state).lock_owned().await;
    let send_transcript = move |text| {
        on_transcript
            .send(text)
            .map_err(|e| format!("Couldn't send a transcript: {e}"))
    };
    spawn_blocking(move || {
        session
            .as_mut()
            .ok_or("Create a speech session before listening.")?
            .start_listening(send_transcript)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Closes the microphone and returns once the trailing speech has been transcribed and sent.
#[tauri::command]
pub async fn stop_listening(state: State<'_, SessionState>) -> Result<(), String> {
    let mut session = Arc::clone(&state).lock_owned().await;
    spawn_blocking(move || {
        session
            .as_mut()
            .ok_or("Create a speech session before listening.")?
            .stop_listening()
    })
    .await
    .map_err(|e| e.to_string())?
}
