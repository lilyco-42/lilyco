mod commands;
mod error;

pub use error::AppError;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::office_commands,
            commands::office_run
        ])
        .run(tauri::generate_context!())
        .expect("lilyco office 起不来");
}
