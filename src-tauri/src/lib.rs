mod speech;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(speech::Speech::default())
        .invoke_handler(tauri::generate_handler![
            speech::start_listening,
            speech::stop_listening
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
