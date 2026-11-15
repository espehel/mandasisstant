mod commands;
mod speech;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(commands::SessionState::default())
        .invoke_handler(tauri::generate_handler![
            commands::create_session,
            commands::start_listening,
            commands::stop_listening
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
